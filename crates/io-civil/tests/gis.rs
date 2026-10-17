//! GIS vector files in and out, CAD linework and base maps — the self-contained checks of the frozen
//! test-civilcraft-gisvec / kml / labels / cad / blocks / maps suites (the GDAL / PROJ comparisons stay in the JS suites;
//! parity with the JS is cvdiff's job). Every fixture is made here, in the test.

use cc_civil::gis_state::{Feature, Field, Geom, Pos};
use cc_civil::js::J;
use cc_io_civil::{basemap, cad_linework, dbf, geojson, gis_export, gis_geom, gis_layers, gis_read, gpkg, kml, shapefile, zipfile};
use serde_json::{Map, Value, json};

const DATE: (i32, u32, u32) = (2024, 5, 17);
const DOS: zipfile::DosDate = zipfile::DosDate { year: 2024, month: 5, day: 17, hour: 13, minute: 45, second: 30 };
const E0: f64 = 3_115_000.0;
const N0: f64 = 10_070_000.0;

fn props(v: Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap_or_default()
}
fn field(name: &str, ty: &str) -> Field {
    Field { name: name.into(), ty: ty.into() }
}
fn sq(x: f64, y: f64, s: f64, z: Option<f64>) -> Vec<Pos> {
    let r = [[x, y], [x + s, y], [x + s, y + s], [x, y + s], [x, y]];
    r.iter()
        .map(|p| match z {
            Some(z) => vec![p[0], p[1], z],
            None => vec![p[0], p[1]],
        })
        .collect()
}
fn rev(mut r: Vec<Pos>) -> Vec<Pos> {
    r.reverse();
    r
}
fn catalog() {
    cc_io_civil::crs::set_catalog();
    cc_io_civil::crs::set_wkt_catalog();
}

struct Sample {
    name: &'static str,
    geom: &'static str,
    has_z: bool,
    fields: Vec<Field>,
    features: Vec<Feature>,
}
fn sample() -> Vec<Sample> {
    vec![
        Sample {
            name: "Valves Ünïcode",
            geom: "pt",
            has_z: true,
            fields: vec![
                field("NAME", "text"),
                field("SIZE_IN", "number"),
                field("COUNT", "number"),
                field("OPEN", "bool"),
                field("SET_ON", "date"),
                field("a_long_field_name_here", "text"),
            ],
            features: vec![
                Feature {
                    g: Some(Geom::Pt(vec![E0 + 1.25, N0 + 2.5, 500.125])),
                    p: props(
                        json!({ "NAME": "V-1 “main” — Straße", "SIZE_IN": 8.5, "COUNT": 3, "OPEN": true, "SET_ON": "2026-10-11", "a_long_field_name_here": "x" }),
                    ),
                },
                Feature {
                    g: Some(Geom::Pt(vec![E0 + 10.0, N0 + 20.0, 501.0])),
                    p: props(
                        json!({ "NAME": "中文", "SIZE_IN": null, "COUNT": -7, "OPEN": false, "SET_ON": null, "a_long_field_name_here": null }),
                    ),
                },
                Feature {
                    g: None,
                    p: props(
                        json!({ "NAME": "no geometry", "SIZE_IN": 1, "COUNT": 0, "OPEN": null, "SET_ON": "2000-01-31", "a_long_field_name_here": "y" }),
                    ),
                },
            ],
        },
        Sample {
            name: "Mains",
            geom: "ln",
            has_z: true,
            fields: vec![field("NAME", "text")],
            features: vec![
                Feature {
                    g: Some(Geom::Ln(vec![
                        vec![vec![E0, N0, 490.0], vec![E0 + 100.0, N0 + 5.0, 489.5], vec![E0 + 180.0, N0 + 40.0, 489.0]],
                        vec![vec![E0 + 5.0, N0 + 60.0, 488.0], vec![E0 + 25.0, N0 + 80.0, 487.75]],
                    ])),
                    p: props(json!({ "NAME": "two parts" })),
                },
                Feature {
                    g: Some(Geom::Ln(vec![vec![vec![E0 - 50.0, N0 - 50.0, 480.0], vec![E0 - 40.0, N0 - 30.0, 481.0]]])),
                    p: props(json!({ "NAME": "one part" })),
                },
            ],
        },
        Sample {
            name: "Lots",
            geom: "pg",
            has_z: false,
            fields: vec![field("LOT", "number"), field("OWNER", "text")],
            features: vec![
                Feature {
                    g: Some(Geom::Pg(vec![vec![
                        sq(E0, N0, 100.0, None),
                        rev(sq(E0 + 20.0, N0 + 20.0, 10.0, None)),
                        rev(sq(E0 + 60.0, N0 + 60.0, 15.0, None)),
                    ]])),
                    p: props(json!({ "LOT": 1, "OWNER": "A" })),
                },
                Feature {
                    g: Some(Geom::Pg(vec![
                        vec![sq(E0 + 200.0, N0, 50.0, None)],
                        vec![sq(E0 + 300.0, N0, 40.0, None), rev(sq(E0 + 310.0, N0 + 10.0, 10.0, None))],
                    ])),
                    p: props(json!({ "LOT": 2, "OWNER": "B & C" })),
                },
            ],
        },
        Sample {
            name: "Pad Z",
            geom: "pg",
            has_z: true,
            fields: vec![field("ELEV", "number")],
            features: vec![Feature {
                g: Some(Geom::Pg(vec![vec![sq(E0 + 400.0, N0 + 400.0, 30.0, Some(505.5))]])),
                p: props(json!({ "ELEV": 505.5 })),
            }],
        },
    ]
}

/// Same geometry up to ring direction / start (shapefiles and GeoJSON re-orient polygon rings).
fn same_geom(a: Option<&Geom>, b: Option<&Geom>) -> bool {
    let p = |x: &Pos, y: &Pos| x.len() == y.len() && x.iter().zip(y).all(|(u, v)| (u - v).abs() <= 1e-9 * v.abs().max(1.0));
    let ring = |x: &[Pos], y: &[Pos]| {
        if x.len() != y.len() || x.is_empty() {
            return x.len() == y.len();
        }
        let xs = &x[..x.len() - 1];
        let ys: Vec<Pos> = y[..y.len() - 1].to_vec();
        let mut ry = ys.clone();
        ry.reverse();
        [ys, ry].iter().any(|r| (0..r.len()).any(|s| xs.iter().enumerate().all(|(i, q)| p(q, &r[(i + s) % r.len()]))))
    };
    match (a, b) {
        (None, None) => true,
        (Some(Geom::Pt(x)), Some(Geom::Pt(y))) => p(x, y),
        (Some(Geom::Ln(x)), Some(Geom::Ln(y))) => {
            x.len() == y.len() && x.iter().zip(y).all(|(u, v)| u.len() == v.len() && u.iter().zip(v).all(|(q, r)| p(q, r)))
        }
        (Some(Geom::Pg(x)), Some(Geom::Pg(y))) => {
            x.len() == y.len() && x.iter().zip(y).all(|(u, v)| u.len() == v.len() && u.iter().zip(v).all(|(q, r)| ring(q, r)))
        }
        _ => false,
    }
}

// ------------------------------------------------------------------------------------------------ zip
#[test]
fn zip_read_and_write() {
    let files = vec![
        ("a.txt".to_string(), "hello ".repeat(1000).into_bytes()),
        ("dir/b.bin".to_string(), vec![0, 1, 2, 3, 255]),
        ("Ünïcode name.txt".to_string(), b"x".to_vec()),
    ];
    let z = zipfile::zip(&files, true, DOS);
    let list = zipfile::list_zip(&z).unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!((list[0].method, list[1].method), (8, 0), "deflated text, stored small binary");
    assert_eq!(zipfile::unzip_entry(&z, &list[0], u64::MAX).unwrap(), files[0].1);
    assert_eq!(zipfile::unzip_entry(&z, &list[1], u64::MAX).unwrap(), vec![0, 1, 2, 3, 255]);
    assert_eq!(list[2].name, "Ünïcode name.txt");
    assert_ne!(list[2].flags & 0x800, 0, "UTF-8 name flag (bit 11)");
    assert_eq!(list[0].crc, zipfile::crc32(&files[0].1));
}

#[test]
fn zip_untrusted() {
    let junk: Vec<u8> = (0..1000u32).map(|i| (i * 7 % 256) as u8).collect();
    assert!(zipfile::list_zip(&junk).unwrap_err().0.contains("not a zip"), "random bytes");
    let z = zipfile::zip(&[("a.txt".to_string(), "hello ".repeat(15).into_bytes())], true, DOS);
    let e = zipfile::list_zip(&z[..z.len() - 10]).unwrap_err().0;
    assert!(e.contains("not a zip") || e.contains("cut short"), "cut short: {e}");
    let zl = zipfile::list_zip(&z).unwrap().remove(0);
    let mut crc = z.clone();
    crc[30 + 5 + 3] ^= 0xff;
    let e = zipfile::unzip_entry(&crc, &zl, u64::MAX).unwrap_err().0;
    assert!(e.contains("damaged") || e.contains("checksum"), "damaged entry: {e}");
    let mut bomb = z.clone();
    bomb[22..26].copy_from_slice(&10u32.to_le_bytes());
    let e = zipfile::unzip_entry(&bomb, &zipfile::Entry { size: 10, ..zl.clone() }, u64::MAX).unwrap_err().0;
    assert!(e.contains("more than it says") || e.contains("different size"), "zip bomb: {e}");
    let e = zipfile::unzip_entry(&z, &zipfile::Entry { size: 3_000_000_000, ..zl.clone() }, 200 * 1024 * 1024).unwrap_err().0;
    assert!(e.contains("MB"), "3 GB entry: {e}");
    assert!(zipfile::unzip_entry(&z, &zipfile::Entry { flags: 1, ..zl.clone() }, u64::MAX).unwrap_err().0.contains("encrypted"));
    assert!(zipfile::unzip_entry(&z, &zipfile::Entry { method: 12, ..zl.clone() }, u64::MAX).unwrap_err().0.contains("method"));
    let mut many = z.clone();
    let eocd = many.len() - 22;
    many[eocd + 10..eocd + 12].copy_from_slice(&0xffffu16.to_le_bytes());
    assert!(zipfile::list_zip(&many).unwrap_err().0.contains("ZIP64"));
}

// ------------------------------------------------------------------------------------------------ dbf
#[test]
fn dbf_write_and_read() {
    let fields = vec![
        field("A_VERY_LONG_FIELD_NAME", "text"),
        field("A_VERY_LONG_FIELD_NAME_2", "number"),
        field("__proto__", "text"),
        field("Höhe m", "number"),
        field("ok", "bool"),
        field("when", "date"),
    ];
    let rec = |a: Value, b: Value, c: Value, h: Value, o: Value, w: Value| {
        props(json!({ "A_VERY_LONG_FIELD_NAME": a, "A_VERY_LONG_FIELD_NAME_2": b, "__proto__": c, "Höhe m": h, "ok": o, "when": w }))
    };
    let recs = [
        rec(json!("é".repeat(200)), json!(1.5), json!("proto"), json!(123.456789), json!(true), json!("2026-01-02")),
        rec(Value::Null, json!(-2), json!(""), Value::Null, json!(false), Value::Null),
        rec(json!("x"), json!(1e7 + 0.125), json!("z"), json!(0.1), Value::Null, json!("1999-12-31")),
    ];
    let refs: Vec<&Map<String, Value>> = recs.iter().collect();
    let (bytes, names) = dbf::write_dbf(&fields, &refs, DATE);
    assert!(names.iter().all(|n| n.len() <= 10 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')), "names: {names:?}");
    let mut up: Vec<String> = names.iter().map(|n| n.to_uppercase()).collect();
    up.sort();
    up.dedup();
    assert_eq!(up.len(), names.len(), "unique names");
    let back = dbf::read_dbf(&bytes, Some("UTF-8"), usize::MAX).unwrap();
    let r = |i: usize, k: usize| back.records[i].get(&names[k]).cloned().unwrap_or(Value::Null);
    assert_eq!(r(0, 0), json!("é".repeat(127)), "cut at 254 bytes on a character boundary");
    assert_eq!((r(0, 1), r(2, 1), r(1, 1)), (json!(1.5), json!(1e7 + 0.125), json!(-2.0)));
    assert_eq!((r(0, 3), r(1, 3)), (json!(123.456789), Value::Null));
    assert_eq!((r(0, 4), r(1, 4), r(2, 4)), (json!(true), json!(false), Value::Null));
    assert_eq!((r(0, 5), r(2, 5)), (json!("2026-01-02"), json!("1999-12-31")));
    assert_eq!(r(0, 2), json!("proto"), "a field named __proto__ is just a field");

    // a header claiming 4 billion records; a header longer than the file
    let mut d2 = bytes.clone();
    d2[4..8].copy_from_slice(&4_000_000_000u32.to_le_bytes());
    let dr = dbf::read_dbf(&d2, Some("UTF-8"), usize::MAX).unwrap();
    assert_eq!(dr.records.len(), 3);
    assert!(dr.notes.iter().any(|n| n.contains("says it has 4000000000 records but holds 3")), "{:?}", dr.notes);
    let mut d3 = bytes.clone();
    d3[8..10].copy_from_slice(&60000u16.to_le_bytes());
    assert!(dbf::read_dbf(&d3, None, usize::MAX).unwrap_err().0.contains("damaged"));
    // __proto__ / constructor(10 chars)
    let pr = props(json!({ "__proto__": "a", "constructor": 1 }));
    let (dp, _) = dbf::write_dbf(&[field("__proto__", "text"), field("constructor", "number")], &[&pr], DATE);
    let dpr = dbf::read_dbf(&dp, Some("UTF-8"), usize::MAX).unwrap();
    assert_eq!(dpr.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["__proto__", "constructo"]);
    assert_eq!(dpr.records[0].get("__proto__"), Some(&json!("a")));
}

#[test]
fn dbf_windows_1252_without_cpg() {
    // a hand-made .dbf: one C(20) field, "Café crème" in Windows-1252, no .cpg → read as Windows-1252, said
    let mut b = vec![0u8; 32];
    b[0] = 3;
    b[4..8].copy_from_slice(&1u32.to_le_bytes());
    b[8..10].copy_from_slice(&65u16.to_le_bytes());
    b[10..12].copy_from_slice(&21u16.to_le_bytes());
    let mut f = vec![0u8; 32];
    f[..4].copy_from_slice(b"NAME");
    f[11] = b'C';
    f[16] = 20;
    b.extend(f);
    b.push(0x0d);
    b.push(b' ');
    let mut v: Vec<u8> = vec![b'C', b'a', b'f', 0xe9, b' ', b'c', b'r', 0xe8, b'm', b'e'];
    v.resize(20, b' ');
    b.extend(v);
    b.push(0x1a);
    let d = dbf::read_dbf(&b, None, usize::MAX).unwrap();
    assert_eq!(d.encoding, "windows-1252");
    assert_eq!(d.records[0].get("NAME"), Some(&json!("Café crème")));
    assert!(d.notes.iter().any(|n| n.contains("Windows-1252")), "{:?}", d.notes);
}

// ------------------------------------------------------------------------------------------------ shapefiles
fn shp_set(name: &str, out: &shapefile::ShpOut, prj: Option<&str>) -> shapefile::ShpSet {
    shapefile::ShpSet {
        name: name.into(),
        shp: Some(out.shp.clone()),
        shx: Some(out.shx.clone()),
        dbf: Some(out.dbf.clone()),
        prj: prj.map(|p| p.as_bytes().to_vec()),
        cpg: Some(b"UTF-8".to_vec()),
    }
}

#[test]
fn shapefiles_round_trip() {
    for s in sample() {
        let out = shapefile::write_shapefile(s.geom, s.has_z, &s.fields, &s.features, DATE);
        let l = shapefile::read_shapefile(&shp_set(s.name, &out, None), usize::MAX).unwrap();
        assert_eq!((l.geom.as_str(), l.has_z), (s.geom, s.has_z), "{}", s.name);
        assert_eq!(l.features.len(), s.features.len(), "{}: count", s.name);
        for (k, (a, b)) in l.features.iter().zip(&s.features).enumerate() {
            assert!(same_geom(a.g.as_ref(), b.g.as_ref()), "{} feature {k}: {:?} vs {:?}", s.name, a.g, b.g);
        }
        if s.geom == "pg" {
            // shapefile winding on read: outer clockwise, holes counter-clockwise
            if let Some(Geom::Pg(c)) = &l.features[0].g {
                assert!(gis_geom::ring_area(&c[0][0]) < 0.0);
                if c[0].len() > 1 {
                    assert!(gis_geom::ring_area(&c[0][1]) > 0.0);
                }
            }
        }
    }
    let s = &sample()[0];
    let out = shapefile::write_shapefile(s.geom, s.has_z, &s.fields, &s.features, DATE);
    let l = shapefile::read_shapefile(&shp_set("v", &out, None), usize::MAX).unwrap();
    assert!(
        l.features[2].g.is_none() && l.features[2].p.get(&out.names[0]) == Some(&json!("no geometry")),
        "an empty geometry is kept with its attributes"
    );
}

#[test]
fn shapefiles_untrusted() {
    let s = &sample()[1];
    let out = shapefile::write_shapefile(s.geom, s.has_z, &s.fields, &s.features, DATE);
    let read = |shp: Vec<u8>, with_dbf: bool| {
        let set =
            shapefile::ShpSet { name: "t".into(), shp: Some(shp), shx: None, dbf: with_dbf.then(|| out.dbf.clone()), prj: None, cpg: None };
        shapefile::read_shapefile(&set, usize::MAX)
    };
    let shp = out.shp.clone();
    let cut = read(shp[..shp.len() - 30].to_vec(), true).unwrap();
    assert_eq!(cut.features.len(), 1);
    assert!(cut.notes.iter().any(|n| n.contains("cut short in shape 2")), "{:?}", cut.notes);
    let mut lie = shp.clone();
    lie[104..108].copy_from_slice(&0x3fff_ffffi32.to_be_bytes());
    let lr = read(lie, true).unwrap();
    assert!(lr.features.is_empty() && lr.notes.iter().any(|n| n.contains("cut short")), "{:?}", lr.notes);
    for (at, v) in [(144usize, 2_000_000_000i32), (148, 0x7fff_ffff), (156, 99)] {
        let mut b = shp.clone();
        b[at..at + 4].copy_from_slice(&v.to_le_bytes());
        assert!(read(b, true).unwrap_err().0.contains("damaged"), "byte {at}");
    }
    let mut hdr = shp.clone();
    hdr[3] = 0;
    assert!(read(hdr, false).unwrap_err().0.contains("not a shapefile"));
    let mut mp = shp.clone();
    mp[32..36].copy_from_slice(&31i32.to_le_bytes());
    assert!(read(mp, false).unwrap_err().0.contains("multipatch"));
    assert!(read(vec![0; 50], false).unwrap_err().0.contains("too short"));
}

// ------------------------------------------------------------------------------------------------ GeoJSON
#[test]
fn geojson_read() {
    let legacy = json!({ "type": "FeatureCollection", "crs": { "type": "name", "properties": { "name": "urn:ogc:def:crs:EPSG::2277" } }, "features": [{ "type": "Feature", "properties": { "a": 1 }, "geometry": { "type": "Point", "coordinates": [3115000, 10070000] } }] });
    assert_eq!(geojson::read_geojson(&legacy.to_string(), "old", usize::MAX).unwrap().crs_code, Some(2277.0), "an old-style crs member");
    let bare = geojson::read_geojson(r#"{"type":"Point","coordinates":[1,2]}"#, "bare", usize::MAX).unwrap();
    assert_eq!(bare.crs_code, None);
    assert_eq!(bare.layers[0].features.len(), 1, "a bare geometry reads as one feature");
    let mixed = json!({ "type": "FeatureCollection", "features": [
        { "type": "Feature", "properties": { "name": "p", "n": 1, "flag": true }, "geometry": { "type": "Point", "coordinates": [-97.74, 30.27, 150.5] } },
        { "type": "Feature", "properties": {}, "geometry": { "type": "MultiPoint", "coordinates": [[-97.7, 30.2], [-97.71, 30.21]] } },
        { "type": "Feature", "properties": {}, "geometry": { "type": "GeometryCollection", "geometries": [{ "type": "LineString", "coordinates": [[0, 0], [1, 1]] }, { "type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 0]]] }] } },
        { "type": "Feature", "properties": { "x": 1 }, "geometry": null }
    ] });
    let r = geojson::read_geojson(&mixed.to_string(), "austin", usize::MAX).unwrap();
    assert_eq!(r.layers.iter().map(|l| l.geom.as_str()).collect::<Vec<_>>(), ["pt", "ln", "pg"], "one layer per kind");
    assert!(
        r.notes.iter().any(|n| n.contains("collections were split")) && r.notes.iter().any(|n| n.contains("multipoints were split")),
        "{:?}",
        r.notes
    );
    assert!(r.layers[0].has_z && matches!(&r.layers[0].features[0].g, Some(Geom::Pt(c)) if c[2] == 150.5));
    let junk = json!({ "type": "FeatureCollection", "features": [
        { "type": "Feature", "geometry": { "type": "Point", "coordinates": ["1", 2] } },
        { "type": "Feature", "geometry": { "type": "LineString", "coordinates": [[0, 0]] } },
        { "type": "Feature", "geometry": { "type": "Polygon", "coordinates": [[[0, 0], [1, 0], [0, 0]]] } },
        null, 5,
        { "type": "Feature", "geometry": { "type": "Point", "coordinates": [1, 2] }, "properties": { "n": 1 } }
    ] });
    let jj = geojson::read_geojson(&junk.to_string(), "j", usize::MAX).unwrap();
    assert_eq!((jj.layers.len(), jj.layers[0].features.len()), (1, 1));
    assert!(jj.notes.iter().any(|n| n.contains("could not be read")), "{:?}", jj.notes);
    assert!(geojson::read_geojson(r#"{"type":"FeatureCollection","features":"#, "x", usize::MAX).unwrap_err().0.contains("not valid JSON"));
    assert!(geojson::read_geojson(r#"{"type":"Nope"}"#, "x", usize::MAX).unwrap_err().0.contains("not GeoJSON"));
    let pp = geojson::read_geojson(
        r#"{"type":"Feature","properties":{"__proto__":{"polluted":1},"constructor":"x"},"geometry":{"type":"Point","coordinates":[1,2]}}"#,
        "p",
        usize::MAX,
    )
    .unwrap();
    assert!(pp.layers[0].features[0].p.get("__proto__").is_some_and(Value::is_string), "__proto__ stays a plain value");
}

#[test]
fn geojson_write_rfc7946() {
    let groups: Vec<geojson::WGroup> = sample()
        .into_iter()
        .map(|s| geojson::WGroup {
            kind: s.name.to_lowercase(),
            geom: s.geom.into(),
            has_z: s.has_z,
            fields: s.fields,
            features: s.features,
        })
        .collect();
    let to_ll = |x: f64, y: f64| (-98.0 + (x - E0) * 1e-6, 31.0 + (y - N0) * 1e-6);
    let text = geojson::write_geojson(&groups, &to_ll, &|z| z * 1200.0 / 3937.0, Some("sample"));
    let j: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(j["type"], "FeatureCollection");
    assert!(j.get("crs").is_none(), "no crs member");
    let fs = j["features"].as_array().unwrap();
    assert!(fs.iter().all(|f| f["properties"]["kind"].is_string()));
    assert!(fs.iter().any(|f| f["geometry"].is_null() && f["properties"]["NAME"] == "no geometry"));
    let mut polys = Vec::new();
    for f in fs {
        match f["geometry"]["type"].as_str() {
            Some("Polygon") => polys.push(f["geometry"]["coordinates"].clone()),
            Some("MultiPolygon") => polys.extend(f["geometry"]["coordinates"].as_array().unwrap().iter().cloned()),
            _ => {}
        }
    }
    assert_eq!(polys.len(), 4);
    for poly in &polys {
        for (i, r) in poly.as_array().unwrap().iter().enumerate() {
            let ring: Vec<Pos> =
                r.as_array().unwrap().iter().map(|p| p.as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect()).collect();
            assert_eq!(gis_geom::ring_area(&ring) > 0.0, i == 0, "right-hand rule");
        }
    }
    let p0 = &fs[0]["geometry"]["coordinates"];
    assert!((p0[2].as_f64().unwrap() - 500.125 * 1200.0 / 3937.0).abs() < 1e-4, "z in metres: {p0}");
}

// ------------------------------------------------------------------------------------------------ KML
const KML_SAMPLE: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2" xmlns:gx="http://www.google.com/kml/ext/2.2">
<Document><name>Survey</name>
 <Placemark><name>Loose &amp; free</name><Point><coordinates>-97.7,30.2,0</coordinates></Point></Placemark>
 <Folder><name>Utilities</name>
  <Placemark><name>MH-1</name><description><![CDATA[<b>Manhole</b> 1]]></description>
   <ExtendedData><Data name="Rim"><value>151.25</value></Data><Data name="Kind"><value>storm</value></Data></ExtendedData>
   <Point><coordinates>-97.7431,30.2672,151.25</coordinates></Point></Placemark>
  <Placemark><name>MH-2</name><ExtendedData><Data name="Rim"><value>n/a</value></Data></ExtendedData><Point><coordinates>-97.7429,30.2673,150</coordinates></Point></Placemark>
  <Placemark><name>Main</name><LineString><coordinates>
    -97.7431,30.2672,151 -97.7425,30.2675,152 -97.7420,30.2672,150.5
  </coordinates></LineString></Placemark>
  <Placemark><name>Both</name><MultiGeometry>
    <LineString><coordinates>-97.74,30.26 -97.741,30.261</coordinates></LineString>
    <Polygon><outerBoundaryIs><LinearRing><coordinates>-97.74,30.26 -97.739,30.26 -97.739,30.261 -97.74,30.26</coordinates></LinearRing></outerBoundaryIs></Polygon>
  </MultiGeometry></Placemark>
  <Placemark><name>Track</name><gx:Track><when>2026-10-12T10:00:00Z</when><gx:coord>-97.74 30.26 150</gx:coord><gx:coord>-97.741 30.262 151</gx:coord></gx:Track></Placemark>
  <Placemark><name>Broken</name><Point><coordinates>500,30.2</coordinates></Point></Placemark>
 </Folder>
 <Folder><name>Lots</name>
  <Placemark><name>Lot 7</name><ExtendedData><SchemaData schemaUrl="#s"><SimpleData name="Area">1234.5</SimpleData></SchemaData></ExtendedData>
   <Polygon><outerBoundaryIs><LinearRing><coordinates>0,0 1,0 1,1 0,1 0,0</coordinates></LinearRing></outerBoundaryIs>
    <innerBoundaryIs><LinearRing><coordinates>0.2,0.2 0.2,0.4 0.4,0.4 0.4,0.2 0.2,0.2</coordinates></LinearRing></innerBoundaryIs></Polygon></Placemark>
 </Folder>
</Document></kml>"##;

#[test]
fn kml_read() {
    let (layers, notes) = kml::read_kml(KML_SAMPLE, "site", usize::MAX).unwrap();
    let by = |n: &str| layers.iter().find(|l| l.name == n).unwrap();
    let mut names: Vec<&str> = layers.iter().map(|l| l.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["Lots", "Survey", "Utilities (lines)", "Utilities (points)", "Utilities (polygons)"]);
    let pts = by("Utilities (points)");
    assert!(pts.features.len() == 2 && pts.has_z && matches!(&pts.features[0].g, Some(Geom::Pt(c)) if c[2] == 151.25));
    assert_eq!(pts.features[0].p.get("description"), Some(&json!("Manhole 1")));
    assert_eq!(pts.features[0].p.get("Kind"), Some(&json!("storm")));
    assert_eq!(pts.fields.iter().find(|f| f.name == "Rim").unwrap().ty, "text", "a non-number keeps the field text");
    assert_eq!(by("Survey").features[0].p.get("name"), Some(&json!("Loose & free")));
    assert!(!by("Survey").has_z, "height 0 reads as 2D");
    let ln = by("Utilities (lines)");
    assert_eq!(ln.features.len(), 3);
    assert!(
        ln.features.iter().any(
            |f| f.p.get("name") == Some(&json!("Track")) && matches!(&f.g, Some(Geom::Ln(c)) if c[0].len() == 2 && c[0][1][2] == 151.0)
        )
    );
    assert_eq!(by("Utilities (polygons)").features.len(), 1);
    let lot = &by("Lots").features[0];
    assert!(matches!(&lot.g, Some(Geom::Pg(c)) if c[0].len() == 2));
    assert_eq!(lot.p.get("Area"), Some(&json!(1234.5)));
    assert_eq!(by("Lots").fields[1].ty, "number");
    assert!(
        notes.iter().any(|n| n.contains("1 placemark geometry had coordinates")) && notes.iter().any(|n| n.contains("MultiGeometry")),
        "{notes:?}"
    );
    assert!(kml::read_kml("<kml><Placemark>", "x", usize::MAX).unwrap_err().0.contains("not a KML file"));
    let (e, _) = kml::read_kml(r#"<!DOCTYPE x [<!ENTITY a "boom">]><kml><Placemark><name>&a;</name><Point><coordinates>1,2</coordinates></Point></Placemark></kml>"#, "e", usize::MAX).unwrap();
    assert_eq!(e[0].features[0].p.get("name"), Some(&json!("&a;")), "a DTD entity is not expanded");
    let (_, capped) = kml::read_kml(KML_SAMPLE, "site", 3).unwrap();
    assert!(capped.iter().any(|n| n.contains("more than 3 placemarks")));
}

#[test]
fn kml_write_and_kmz_read_back() {
    catalog();
    let s = &sample()[0];
    let to_ll = |x: f64, y: f64| (-98.0 + (x - E0) * 1e-6, 31.0 + (y - N0) * 1e-6);
    let text = kml::write_kml(&[kml::KGroup { kind: "valves", has_z: true, features: &s.features }], &to_ll, &|z| z * 0.3048, "site");
    assert!(text.starts_with("<?xml"));
    let kmz = zipfile::zip(&[("doc.kml".to_string(), text.into_bytes())], true, DOS);
    let (sources, _) = gis_read::read_gis_files(&[("site.kmz".to_string(), kmz)], 500_000);
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].format, "KML");
    assert!(matches!(sources[0].src, gis_read::Src::Wgs84));
    let f0 = &sources[0].layers[0].features[0];
    let Some(Geom::Pt(c)) = &f0.g else { panic!("a point") };
    let ll = to_ll(E0 + 1.25, N0 + 2.5);
    assert!((c[0] - ll.0).abs() < 1e-8 && (c[1] - ll.1).abs() < 1e-8 && (c[2] - 500.125 * 0.3048).abs() < 1e-3);
}

// ------------------------------------------------------------------------------------------------ GeoPackage
#[test]
fn geopackage_round_trip() {
    let s = sample();
    let groups: Vec<gpkg::GGroup> = s
        .iter()
        .map(|l| gpkg::GGroup { table: l.name, label: l.name, geom: l.geom, has_z: l.has_z, fields: &l.fields, features: &l.features })
        .collect();
    let srs = gpkg::Srs {
        id: 2277.0,
        org: "EPSG".into(),
        code: Some(2277.0),
        name: "NAD83 / Texas Central (ftUS)".into(),
        definition: "undefined".into(),
    };
    let bytes = gpkg::write_gpkg(&groups, Some(&srs)).unwrap();
    let gp = gpkg::open_gpkg(&bytes).unwrap();
    assert_eq!(gp.tables.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), s.iter().map(|l| l.name).collect::<Vec<_>>());
    assert!(gp.tables.iter().all(|t| t.srs_id == 2277.0));
    for (t, l) in gp.tables.iter().zip(&s) {
        let ls = gp.read(&t.name, usize::MAX).unwrap();
        assert_eq!(ls[0].features.len(), l.features.len(), "{}", l.name);
        for (a, b) in ls[0].features.iter().zip(&l.features) {
            assert!(same_geom(a.g.as_ref(), b.g.as_ref()), "{}: {:?} vs {:?}", l.name, a.g, b.g);
            for f in &l.fields {
                let (x, y) = (a.p.get(&f.name).cloned().unwrap_or(Value::Null), b.p.get(&f.name).cloned().unwrap_or(Value::Null));
                let same = match (x.as_f64(), y.as_f64()) {
                    (Some(u), Some(v)) => u == v,
                    _ => x == y || (f.ty == "bool" && x.as_f64().map(|u| u != 0.0) == y.as_bool()),
                };
                assert!(same, "{}.{}: {x} vs {y}", l.name, f.name);
            }
        }
    }
    // readGisFiles: one source per table, systems resolved
    catalog();
    let (src, _) = gis_read::read_gis_files(&[("site.gpkg".to_string(), bytes)], 500_000);
    assert_eq!(src.len(), 4);
    assert!(src.iter().all(|x| matches!(&x.src, gis_read::Src::Crs { matched: Some(m), .. } if m["code"].as_f64() == Some(2277.0))));
}

#[test]
fn geopackage_untrusted() {
    assert!(gpkg::open_gpkg(&vec![0u8; 5000]).err().unwrap().0.contains("not a GeoPackage"));
    let f = vec![field("x\"y", "text")];
    let feats = vec![
        Feature { g: Some(Geom::Pt(vec![1.0, 2.0])), p: props(json!({ "x\"y": "v" })) },
        Feature { g: Some(Geom::Pt(vec![3.0, 4.0])), p: props(json!({ "x\"y": "w" })) },
    ];
    let bytes = gpkg::write_gpkg(
        &[gpkg::GGroup { table: "we\"ird name", label: "odd", geom: "pt", has_z: false, fields: &f, features: &feats }],
        None,
    )
    .unwrap();
    let gp = gpkg::open_gpkg(&bytes).unwrap();
    let ls = gp.read("we\"ird name", usize::MAX).unwrap();
    assert_eq!(ls[0].features[1].p.get("x\"y"), Some(&json!("w")), "a table and a field with a quote in their names");
    let e = gpkg::open_gpkg(&bytes[..2048]).err().unwrap().0;
    assert!(e.contains("damaged") || e.contains("not a GeoPackage") || e.contains("could not"), "cut short: {e}");
    assert!(gpkg::parse_gpb(&[0x47, 0x50, 0, 1, 0, 0, 0, 0, 0xff, 0xff]).is_err(), "a damaged geometry blob");
}

// ------------------------------------------------------------------------------------------------ WKB
#[test]
fn wkb_untrusted() {
    let mut huge = vec![0u8; 13];
    huge[0] = 1;
    huge[1..5].copy_from_slice(&2u32.to_le_bytes());
    huge[5..9].copy_from_slice(&0xffff_fff0u32.to_le_bytes());
    let e = gis_geom::read_wkb(&huge, 0).unwrap_err().0;
    assert!(e.contains("more points than") || e.contains("cut short"), "{e}");
    let mut deep: Vec<u8> = Vec::new();
    for _ in 0..12 {
        let mut h = vec![1u8, 7, 0, 0, 0, 1, 0, 0, 0];
        h.extend(deep);
        deep = h;
    }
    let e = gis_geom::read_wkb(&deep, 0).unwrap_err().0;
    assert!(e.contains("nested too deeply") || e.contains("cut short"), "{e}");
    for s in sample() {
        for f in &s.features {
            if f.g.is_none() {
                assert!(gis_geom::write_wkb(None, s.has_z).is_err(), "no geometry to write");
                continue;
            }
            let w = gis_geom::write_wkb(f.g.as_ref(), s.has_z).unwrap();
            let (g, _, _) = gis_geom::read_wkb(&w, 0).unwrap();
            assert!(same_geom(g[0].as_ref(), f.g.as_ref()), "{}", s.name);
        }
    }
}

// ------------------------------------------------------------------------------------------------ export + import
fn egroups(s: &[Sample]) -> Vec<gis_export::EGroup> {
    let mut used = Vec::new();
    s.iter()
        .map(|l| gis_export::EGroup {
            key: format!("gis:{}", l.name),
            label: l.name.into(),
            kind: gis_export::safe_name(l.name, &mut Vec::new()),
            file: gis_export::safe_name(l.name, &mut used),
            geom: l.geom.into(),
            has_z: l.has_z,
            fields: l.fields.clone(),
            features: l.features.clone(),
        })
        .collect()
}

#[test]
fn export_shapefiles_and_read_back() {
    catalog();
    let crs = cc_io_civil::crs::crs_from_entry(cc_io_civil::crs::by_code(2277.0).unwrap().unwrap());
    let prj = cc_io_civil::crs::prj_for(&crs).unwrap();
    let s = sample();
    let groups = egroups(&s);
    let x = gis_export::write_export(&groups, "shp", Some(&prj), None, &|z| z, "sample", None, DOS).unwrap();
    assert_eq!(x.name, "sample-gis.zip");
    let list = zipfile::list_zip(&x.bytes).unwrap();
    for g in &groups {
        for e in ["shp", "shx", "dbf", "prj", "cpg"] {
            assert!(list.iter().any(|z| z.name == format!("{}.{e}", g.file)), "{}.{e}", g.file);
        }
    }
    assert!(x.notes.iter().any(|n| n.contains("shortened") && n.contains("a_long_field_name_here")), "{:?}", x.notes);
    let (src, _) = gis_read::read_gis_files(&[(x.name.clone(), x.bytes.clone())], 500_000);
    assert_eq!(src.len(), 4);
    assert!(
        src.iter().all(|x| matches!(&x.src, gis_read::Src::Crs { matched: Some(m), .. } if m["code"].as_f64() == Some(2277.0))),
        ".prj → EPSG:2277"
    );
    // our reader sorts shapefile sets by name; match them back by file
    for (g, l) in groups.iter().zip(&s) {
        let so = src
            .iter()
            .find(|x| x.file == format!("{}.shp", g.file))
            .unwrap_or_else(|| panic!("{} in {:?}", g.file, src.iter().map(|x| &x.file).collect::<Vec<_>>()));
        assert!(so.layers[0].features.iter().zip(&l.features).all(|(a, b)| same_geom(a.g.as_ref(), b.g.as_ref())), "{}", g.file);
    }
    // the import decision for this zip
    let plan = gis_read::plan_import(&Value::Null, "ft", &src);
    assert_eq!(plan["case"], "offer");
    assert_eq!(plan["offer"]["code"].as_f64(), Some(2277.0));
    let plan_m = gis_read::plan_import(&Value::Null, "m", &src);
    assert_eq!(plan_m["case"], "pick");
    assert!(plan_m["why"].as_str().unwrap().contains("feet"), "{}", plan_m["why"]);
    // placed into a 2277 project: same system, coordinates unchanged
    let (layers, _) = gis_read::build_layers(&src, &crs, "ft", &[]);
    assert_eq!(layers.len(), 4);
    // no project: never placed
    let (none, notes) = gis_read::build_layers(&src, &Value::Null, "ft", &[]);
    assert!(none.is_empty() && !notes.is_empty());
    // no prj: no system
    let y = gis_export::write_export(&groups[..1], "shp", None, None, &|z| z, "s", None, DOS).unwrap();
    assert!(!zipfile::list_zip(&y.bytes).unwrap().iter().any(|e| e.name.ends_with(".prj")));
    let (src2, _) = gis_read::read_gis_files(&[(y.name, y.bytes)], 500_000);
    assert!(matches!(src2[0].src, gis_read::Src::None));
    assert_eq!(gis_read::plan_import(&Value::Null, "ft", &src2)["case"], "none");
    // GeoJSON / KML without a system are refused; an unknown format too
    assert!(gis_export::write_export(&groups, "geojson", None, None, &|z| z, "s", None, DOS).is_err());
    assert!(gis_export::write_export(&groups, "kml", None, None, &|z| z, "s", None, DOS).is_err());
    assert!(gis_export::write_export(&[], "shp", None, None, &|z| z, "s", None, DOS).is_err());
}

#[test]
fn import_reprojects_and_refuses() {
    catalog();
    let crs = cc_io_civil::crs::crs_from_entry(cc_io_civil::crs::by_code(2277.0).unwrap().unwrap());
    let gj = json!({ "type": "FeatureCollection", "features": [
        { "type": "Feature", "properties": {}, "geometry": { "type": "Point", "coordinates": [-97.7431, 30.2672, 150.5] } },
        { "type": "Feature", "properties": {}, "geometry": { "type": "Point", "coordinates": [3115000, 10070000] } }
    ] });
    let (src, _) = gis_read::read_gis_files(&[("a.geojson".to_string(), gj.to_string().into_bytes())], 500_000);
    let near = gis_read::middle_lon_lat(&src);
    assert!(near.is_some());
    let (layers, notes) = gis_read::build_layers(&src, &crs, "ft", &[]);
    let j = layers.j();
    let fs = j[0]["features"].as_array().unwrap();
    assert_eq!(fs.len(), 1, "the position outside ±180/±90 is refused");
    assert!(notes.iter().any(|n| n.contains("outside")), "{notes:?}");
    let c = &fs[0]["g"]["c"];
    // Austin in Texas Central (ftUS): about E 3.11e6, N 1.007e7; z 150.5 m in US survey feet
    assert!((c[0].as_f64().unwrap() - 3_114_000.0).abs() < 10_000.0 && (c[1].as_f64().unwrap() - 10_070_000.0).abs() < 10_000.0, "{c}");
    assert!((c[2].as_f64().unwrap() - 150.5 / (1200.0 / 3937.0)).abs() < 1e-9);
    assert!(
        notes.iter().any(|n| n.contains("elevations converted from metres")) && notes.iter().any(|n| n.contains("converted from WGS 84")),
        "{notes:?}"
    );
    // latitude/longitude point rows
    let pts = vec![
        cc_civil::points::read_pt(&json!({ "n": "1", "x": -97.7431, "y": 30.2672, "z": 500.5, "d": "CP" })),
        cc_civil::points::read_pt(&json!({ "n": "2", "x": 400, "y": 95, "z": null, "d": "" })),
    ];
    let (p, bad) = gis_read::lat_lon_points(&pts, &crs).unwrap();
    assert_eq!((p.len(), bad), (1, 1));
    assert!((p[0].x - c[0].as_f64().unwrap()).abs() < 1e-6 && p[0].z == Some(500.5));
    // LandXML <CoordinateSystem>
    let (lx, _) = gis_read::crs_from_landxml(Some(&json!({ "epsgCode": 2277 })));
    assert_eq!(lx["code"].as_f64(), Some(2277.0));
    let (lx, why) = gis_read::crs_from_landxml(Some(&json!({ "epsgCode": 999_999, "name": "" })));
    assert!(lx.is_null() && !why.is_empty());
    assert_eq!(gis_read::gis_file_kind("x.json", r#"{"type": "FeatureCollection""#), Some("geojson"));
    assert_eq!(gis_read::gis_file_kind("x.json", r#"{"format":"civilcraft-project""#), None, "a project file is not GeoJSON");
    let (none, notes) = gis_read::read_gis_files(&[("junk.zip".to_string(), vec![0; 500])], 500_000);
    assert!(none.is_empty() && notes[0].contains("junk.zip: This is not a zip"), "{notes:?}");
}

// ------------------------------------------------------------------------------------------------ the layer model
#[test]
fn layers_pick_scene_extents_labels() {
    let (se, sn) = (6_150_000.0, 2_080_000.0);
    let line = |i: f64| json!([{ "t": "ln", "c": [[[se + 50.0 + i * 30.0, sn + 50.0], [se + 60.0 + i * 30.0, sn + 150.0], [se + 55.0 + i * 30.0, sn + 250.0]]] }, { "ELEV": 252.0 + i, "NOTE": format!("c{i}") }]);
    let gis = json!({ "layers": [
        { "id": "g1", "name": "Contours", "geom": "ln", "hasZ": false, "fields": [{ "name": "ELEV", "type": "number" }, { "name": "NOTE", "type": "text" }], "label": { "field": "NOTE" } },
        { "id": "g2", "name": "Break Z", "geom": "ln", "hasZ": true, "fields": [] },
        { "id": "g3", "name": "Site", "geom": "pg", "hasZ": false, "fields": [] },
        { "id": "g4", "name": "Shots", "geom": "pt", "hasZ": true, "fields": [{ "name": "H", "type": "number" }], "label": { "field": "H" } }
    ], "maps": [] });
    let feats = json!({
        "g1": [line(0.0), line(1.0), line(2.0)],
        "g2": [[{ "t": "ln", "c": [[[se + 20.0, sn + 20.0, 249], [se + 380.0, sn + 280.0, 258]]] }, {}]],
        "g3": [[{ "t": "pg", "c": [[[[se + 10.0, sn + 10.0], [se + 390.0, sn + 10.0], [se + 390.0, sn + 290.0], [se + 10.0, sn + 290.0], [se + 10.0, sn + 10.0]]]] }, {}],
               [{ "t": "pg", "c": [[[[se + 100.0, sn + 100.0], [se + 120.0, sn + 100.0], [se + 120.0, sn + 120.0], [se + 100.0, sn + 120.0], [se + 100.0, sn + 100.0]]]] }, {}]],
        "g4": [[{ "t": "pt", "c": [se + 200.0, sn + 150.0, 260.5] }, { "H": 999 }], [{ "t": "pt", "c": [se + 210.0, sn + 150.0] }, { "H": 261 }]]
    });
    let (g, fs) = cc_civil::gis_state::read_gis_state(Some(&gis), Some(&feats), &mut Vec::new());
    let ext = gis_layers::gis_extents(&g.layers, &fs).unwrap();
    assert_eq!((ext.min_x, ext.max_y), (se + 10.0, sn + 290.0));
    let sc = gis_layers::gis_scene(&g.layers, &fs, 1.0);
    assert_eq!(
        sc["layers"].as_array().unwrap().iter().map(|l| l["name"].as_str().unwrap()).collect::<Vec<_>>(),
        ["GIS-g1", "GIS-g2", "GIS-g3", "GIS-g4"]
    );
    let items = sc["items"].as_array().unwrap();
    let polys: Vec<&Value> = items.iter().filter(|i| i["kind"] == "poly").collect();
    assert_eq!(polys.len(), 3 + 1 + 2 + 2, "lines, ring outlines, point squares");
    assert_eq!(items.iter().filter(|i| i["kind"] == "text").count(), 3 + 2, "labels");
    assert!(polys.iter().any(|i| i["layer"] == "GIS-g4" && i["pts"].as_array().unwrap().len() == 4));
    assert!(polys.iter().any(|i| i["layer"] == "GIS-g2" && i["z"] == true));
    assert_eq!(
        gis_layers::pick_feature(&g.layers, &fs, se + 90.0, sn + 150.0, 2.0),
        Some(("g1".into(), 1)),
        "the second contour by its line"
    );
    assert_eq!(
        gis_layers::pick_feature(&g.layers, &fs, se + 110.0, sn + 110.0, 0.5),
        Some(("g3".into(), 1)),
        "inside two polygons: the smaller"
    );
    assert_eq!(gis_layers::pick_feature(&g.layers, &fs, se + 200.4, sn + 150.3, 1.0), Some(("g4".into(), 0)), "a point");
    let g1 = &g.layers[0];
    let ch = gis_layers::use_choices(g1, gis_layers::features_of(&fs, "g1"));
    assert_eq!(ch.len(), 1);
    assert_eq!((ch[0]["from"].as_str(), ch[0]["fields"].clone()), (Some("field"), json!(["ELEV"])));
    // a hidden layer is neither drawn nor picked
    let mut hidden = gis.clone();
    hidden["layers"][3]["visible"] = json!(false);
    let (h, hf) = cc_civil::gis_state::read_gis_state(Some(&hidden), Some(&feats), &mut Vec::new());
    assert_ne!(gis_layers::pick_feature(&h.layers, &hf, se + 200.4, sn + 150.3, 1.0).map(|x| x.0), Some("g4".into()));
    assert!(!gis_layers::gis_scene(&h.layers, &hf, 1.0)["layers"].as_array().unwrap().iter().any(|l| l["name"] == "GIS-g4"));
    // labels
    assert_eq!(gis_layers::label_text(Some(&json!(1.23456))), "1.235");
    assert_eq!(gis_layers::label_text(Some(&json!("  a \n b "))), "a b");
    assert_eq!(gis_layers::label_text(Some(&Value::Null)), "");
    assert_eq!(gis_layers::label_text(Some(&json!("x".repeat(100)))).len(), 80);
}

#[test]
fn safe_names() {
    let mut used = Vec::new();
    assert_eq!(gis_export::safe_name("Road edges", &mut used), "Road_edges");
    assert_eq!(gis_export::safe_name("road edges", &mut used), "road_edges_2");
    assert_eq!(gis_export::safe_name("1st floor", &mut used), "l_1st_floor");
    assert_eq!(gis_export::safe_name("***", &mut used), "layer");
    assert_eq!(gis_export::safe_name("Ñandú", &mut used), "N_andu");
}

// ------------------------------------------------------------------------------------------------ CAD linework
const DXF: &str = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n2\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n\
0\nLINE\n8\nROAD\n10\n0\n20\n0\n30\n0\n11\n100\n21\n0\n31\n0\n\
0\nLWPOLYLINE\n8\nLOT\n90\n4\n70\n1\n10\n0\n20\n0\n10\n10\n20\n0\n10\n10\n20\n10\n10\n0\n20\n10\n\
0\nCIRCLE\n8\nTREE\n10\n5\n20\n5\n30\n0\n40\n1\n\
0\nTEXT\n8\nNOTES\n10\n1\n20\n1\n40\n2\n1\nhello\n0\nENDSEC\n0\nEOF\n";

#[test]
fn cad_linework_from_dxf() {
    let m = cc_io_dwg::dxf_model::try_read_dxf_model(DXF).unwrap();
    let header = match m.get("header") {
        Some(cc_io_dwg::V::Obj(h)) => Some(h),
        _ => None,
    };
    let u = cad_linework::cad_units(header);
    assert_eq!(u.j()["label"], "ft", "{}", u.j());
    let s = cad_linework::unit_scale(header, "m");
    assert!((s.j()["scale"].as_f64().unwrap() - 0.3048).abs() < 1e-12, "{}", s.j());
    let lw = cad_linework::cad_linework(&m, "x.dxf", "ft", None, None).j();
    let names: Vec<&str> = lw["layers"].as_array().unwrap().iter().map(|l| l["name"].as_str().unwrap()).collect();
    assert!(
        names.iter().any(|n| n.contains("ROAD")) && names.iter().any(|n| n.contains("LOT")) && names.iter().any(|n| n.contains("TREE")),
        "{names:?} / {lw}"
    );
    // text is not linework
    assert!(!names.iter().any(|n| n.contains("NOTES")), "{names:?}");
    // only the layers asked for
    let only = cad_linework::cad_linework(&m, "x.dxf", "ft", None, Some(&["ROAD".to_string()])).j();
    assert!(only["layers"].as_array().unwrap().iter().all(|l| l["name"].as_str().unwrap().contains("ROAD")), "{only}");
}

// ------------------------------------------------------------------------------------------------ base maps
#[test]
fn base_maps() {
    let (x, y) = basemap::lon_lat_to_merc(-97.7431, 30.2672);
    let (lon, lat) = basemap::merc_to_lon_lat(x, y);
    assert!((lon + 97.7431).abs() < 1e-9 && (lat - 30.2672).abs() < 1e-9, "Web Mercator round trip");
    assert!((basemap::lon_lat_to_merc(180.0, 0.0).0 - 20_037_508.342_789_244).abs() < 1e-6);
    let (tx, ty) = basemap::tile_at(0.0, 0.0, 1);
    assert_eq!((tx, ty), (1.0, 1.0));
    assert_eq!(basemap::code_of_crs("EPSG:3857").map(|c| c.0), Some(3857.0));
    assert_eq!(basemap::code_of_crs("urn:ogc:def:crs:EPSG::4326").map(|c| c.0), Some(4326.0));
    assert_eq!(basemap::code_of_crs("CRS:84").map(|c| (c.0, c.1)), Some((4326.0, true)));
    assert!(
        basemap::is_lat_first(Some(4326.0), false)
            && !basemap::is_lat_first(Some(4326.0), true)
            && !basemap::is_lat_first(Some(3857.0), false)
    );
    let wf = basemap::parse_world_file("0.5\n0\n0\n-0.5\n3115000.25\n10070000.25\n").unwrap();
    // GDAL geotransform order, from pixel centres to pixel corners
    assert_eq!(wf, [3_115_000.0, 0.5, 0.0, 10_070_000.5, 0.0, -0.5]);
    assert!(basemap::parse_world_file("1\n2\n3\n").is_err());
    assert!(basemap::world_file_for("site.png").contains(&"site.pgw".to_string()));
    assert!(basemap::world_file_for("site.jpg").contains(&"site.jgw".to_string()));
    let m = basemap::affine_from3([[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]], [[10.0, 20.0], [12.0, 20.0], [10.0, 23.0]]).unwrap();
    assert_eq!(basemap::apply_affine(&m, 1.0, 1.0), [12.0, 23.0]);
    assert!(
        basemap::affine_from3([[0.0, 0.0], [1.0, 1.0], [2.0, 2.0]], [[0.0, 0.0], [1.0, 1.0], [2.0, 2.0]]).is_none(),
        "collinear points"
    );
    assert!(basemap::check_url("javascript:alert(1)").is_err());
    assert!(basemap::check_url("https://tiles.example.com/x").is_ok());
    assert!(basemap::check_template("https://tile.example.com/{z}/{x}/{y}.png").is_ok());
    assert!(basemap::check_template("https://tile.example.com/static.png").is_err(), "a template without {{z}}/{{x}}/{{y}}");
    let g = basemap::xyz_grid();
    let (mx, tiles) = basemap::grid_tiles(&g, [-1000.0, -1000.0, 1000.0, 1000.0], 10.0, 64.0);
    assert!(mx.is_some() && !tiles.is_empty() && tiles.len() <= 64);
}
