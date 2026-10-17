//! Coordinate systems: the fixed cases of the 0.5 test-civilcraft-crs.mjs that need no second reader (PROJ), plus a
//! generated NTv2 grid. Values that come from the JS build are noted where they are used.

use cc_io_civil::{crs, crs_pick};
use serde_json::{Value, json};

fn entry(code: f64) -> &'static crs::Entry {
    crs::set_catalog();
    crs::by_code(code).unwrap().unwrap()
}

#[test]
fn catalog_and_lookup() {
    let cat = crs::set_catalog();
    assert!(cat.len() > 5000);
    for c in [2277.0, 2278.0, 32615.0, 32755.0, 2154.0, 27700.0, 7856.0, 2193.0, 3857.0, 25832.0, 28992.0] {
        assert!(crs::by_code(c).unwrap().is_some(), "EPSG:{c}");
    }
    assert!(crs::by_code(999999.0).unwrap().is_none());
    assert_eq!(crs::by_code_val(Some(&json!("2277"))).unwrap().unwrap().code, 2277.0);
}

#[test]
fn search_and_suggest() {
    crs::set_catalog();
    assert_eq!(crs::search("2277", None, 50.0).unwrap()[0].code, 2277.0);
    assert_eq!(crs::search("EPSG:2277", None, 50.0).unwrap()[0].code, 2277.0);
    assert_eq!(crs::search("epsg: 27700", None, 50.0).unwrap()[0].code, 27700.0);
    assert!(crs::search("22", None, 50.0).unwrap().iter().all(|e| cc_civil::js::num_to_string(e.code).starts_with("22")));
    assert!(crs::search("no such system at all", None, 50.0).unwrap().is_empty());
    assert_eq!(crs::search("", None, 3.0).unwrap().len(), 3);
    let near = crs::search("utm", Some([-97.7, 30.3]), 5.0).unwrap();
    assert!(near.iter().all(|e| e.area.is_some_and(|a| a[0] <= -97.7 && a[2] >= -97.7)));
    let has = |l: Vec<&crs::Entry>, c: f64| l.iter().any(|e| e.code == c);
    assert!(has(crs::suggest(2.35, 48.85, Some("m"), 12.0).unwrap(), 2154.0), "Paris → Lambert-93");
    assert!(has(crs::suggest(174.78, -41.29, Some("m"), 12.0).unwrap(), 2193.0), "Wellington → NZTM");
    assert!(has(crs::suggest(151.2, -33.87, Some("m"), 12.0).unwrap(), 7856.0), "Sydney → MGA 56");
    assert!(has(crs::suggest(-0.13, 51.5, Some("m"), 12.0).unwrap(), 27700.0), "London → BNG");
}

#[test]
fn frame_round_trips_and_local_steps() {
    let base = crs::crs_from_entry(entry(2277.0));
    let f = crs::frame(&base).unwrap().unwrap();
    let xy = f.from_lon_lat(-97.7, 30.3).unwrap();
    let ll = crs::nn(f.to_lon_lat(xy[0], xy[1]).unwrap());
    assert!((ll[0] + 97.7).abs() < 1e-9 && (ll[1] - 30.3).abs() < 1e-9);
    // local fit from two control points reproduces them
    let truth = crs::Local { scale: 0.99987, rotation: 12.5, from: [5000.0, 5000.0], to: [3115000.0, 10070000.0] };
    let a = [5000.0, 5000.0];
    let b = [5800.0, 5300.0];
    let pairs = json!([[a, crs_pick::local_to_grid(&truth, a[0], a[1])], [b, crs_pick::local_to_grid(&truth, b[0], b[1])]]);
    let fit = crs::local_from_pairs(&pairs).unwrap();
    assert!((fit.scale - truth.scale).abs() < 1e-12 && (fit.rotation - truth.rotation).abs() < 1e-9);
    let m = crs_pick::misclosure(&fit, [5400.0, 5900.0], crs_pick::local_to_grid(&truth, 5400.0, 5900.0));
    assert!(m["dist"].as_f64().unwrap() < 1e-6);
    assert!(crs::local_from_pairs(&json!([[a, [0, 0]], [a, [1, 1]]])).is_err());
    assert!(crs::local_from_pairs(&json!([[a, [0, 0]]])).is_err());
    let g = crs::ground_local(0.99987, [0.0, 0.0]);
    let mut c = base.clone();
    c["local"] = cc_civil::js::J::j(&g);
    assert_eq!(crs::crs_problem(&c, "ft"), None);
    let fr = crs::frame(&c).unwrap().unwrap();
    let p = fr.from_grid(1000.0, 2000.0);
    let q = fr.to_grid(p[0], p[1]);
    assert!((q[0] - 1000.0).abs() < 1e-9 && (q[1] - 2000.0).abs() < 1e-9);
}

#[test]
fn problems() {
    let ftus = crs::crs_from_entry(entry(2277.0));
    let m = crs::crs_from_entry(entry(32614.0));
    assert_eq!(crs::crs_problem(&ftus, "ft"), None);
    assert!(crs::crs_problem(&m, "ft").unwrap().contains("met"));
    assert!(crs::crs_problem(&ftus, "m").unwrap().contains("feet"));
    assert!(crs::crs_problem(&json!({"code": null, "name": "x", "proj4": "", "unit": "m"}), "m").unwrap().contains("definition"));
    assert!(
        crs::crs_problem(&json!({"code": null, "name": "x", "proj4": "+proj=nonsense +units=m", "unit": "m"}), "m")
            .unwrap()
            .contains("could not be used")
    );
    assert_eq!(crs::crs_problem(&Value::Null, "ft"), None);
    let mut z = ftus.clone();
    z["local"] = json!({"scale": 0, "rotation": 0, "from": [0, 0], "to": [0, 0]});
    assert!(crs::crs_problem(&z, "ft").unwrap().contains("not valid"));
}

#[test]
fn prj_files() {
    crs::set_catalog();
    let auth = crs::crs_from_prj(r#"PROJCS["anything",GEOGCS["x",DATUM["y",SPHEROID["z",6378137,298.257223563]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433]],PROJECTION["Transverse_Mercator"],PARAMETER["central_meridian",-93],PARAMETER["scale_factor",0.9996],PARAMETER["false_easting",500000],PARAMETER["false_northing",0],PARAMETER["latitude_of_origin",0],UNIT["metre",1],AUTHORITY["EPSG","32615"]]"#).unwrap();
    assert_eq!(auth.matched.unwrap().code, 32615.0);
    let g = crs::crs_from_prj(r#"GEOGCS["GCS_North_American_1983",DATUM["D_North_American_1983",SPHEROID["GRS_1980",6378137.0,298.257222101]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]]"#).unwrap();
    assert_eq!(g.matched.unwrap().code, 4269.0);
    let own = r#"PROJCS["Site grid",GEOGCS["GCS_WGS_1984",DATUM["D_WGS_1984",SPHEROID["WGS_1984",6378137.0,298.257223563]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]],PROJECTION["Transverse_Mercator"],PARAMETER["False_Easting",1000.0],PARAMETER["False_Northing",2000.0],PARAMETER["Central_Meridian",-97.123],PARAMETER["Scale_Factor",0.99991],PARAMETER["Latitude_Of_Origin",30.0],UNIT["Meter",1.0]]"#;
    let r = crs::crs_from_prj(own).unwrap();
    assert!(r.matched.is_none() && r.crs["name"] == "Site grid" && r.crs["unit"] == "m" && r.notes.len() == 1);
    assert_eq!(crs::prj_for(&r.crs).as_deref(), Some(own));
    for bad in ["", "hello world", "LOCAL_CS[\"Site\",LOCAL_DATUM[\"x\",0],UNIT[\"Meter\",1]]"] {
        assert!(crs::crs_from_prj(bad).is_err(), "{bad}");
    }
    assert!(crs::crs_from_prj(&format!("PROJCS[{}]", "x".repeat(30000))).is_err());
    // prjFor of a catalog system reads back as that system
    for c in [2277.0, 32615.0, 27700.0, 2193.0] {
        let w = crs::prj_for(&crs::crs_from_entry(entry(c))).unwrap();
        assert_eq!(crs::crs_from_prj(&w).unwrap().matched.unwrap().code, c);
    }
}

#[test]
fn words_and_lat_long() {
    crs::set_catalog();
    assert_eq!(
        crs_pick::area_words(Some(&json!([-106.66, 29.78, -93.5, 32.27]))),
        "Latitude 29.78° N to 32.27° N, longitude 106.66° W to 93.50° W"
    );
    assert_eq!(crs_pick::area_words(Some(&json!([-180, -90, 180, 90]))), "The whole world");
    assert_eq!(crs_pick::area_words(None), "Not stated");
    assert_eq!(crs_pick::lat_lon_text(-97.7, 30.3, "dms"), "30°18′00.00″ N  97°42′00.00″ W");
    assert_eq!(crs_pick::lat_lon_text(151.2093, -33.8688, "dd"), "33.868800° S  151.209300° E");
    assert_eq!(crs_pick::lat_lon_text(-0.0000001, 59.99999999, "dms"), "60°00′00.00″ N  0°00′00.00″ W");
    assert_eq!(crs_pick::lat_lon_text(f64::NAN, 1.0, "dms"), "");
    let near = |a: Option<[f64; 2]>, b: [f64; 2]| a.is_some_and(|a| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9);
    assert!(near(crs_pick::parse_lat_lon("30.3, -97.7"), [-97.7, 30.3]));
    assert!(near(crs_pick::parse_lat_lon("30°18'00\"N 97°42'00\"W"), [-97.7, 30.3]));
    assert!(near(crs_pick::parse_lat_lon("N30 18 00 W97 42 00"), [-97.7, 30.3]));
    assert!(near(crs_pick::parse_lat_lon("97.7W, 30.3N"), [-97.7, 30.3]));
    assert!(near(crs_pick::parse_lat_lon("-33.8688; 151.2093"), [151.2093, -33.8688]));
    for bad in ["", "junk", "95, 10", "10, 200", "30 70 00 N, 97 W", "1, 2, 3", "N30 E97 S10", "30.3N, 97.7N"] {
        assert!(crs_pick::parse_lat_lon(bad).is_none(), "{bad}");
    }
    assert!(crs_pick::shift_words(&json!({"code": 1, "name": "X / Y", "shiftAccuracy": null})).contains("not known"));
    assert!(crs_pick::shift_words(&crs::Entry::value(entry(32614.0))).contains("WGS 84 itself"));
}

#[test]
fn systems_that_fit() {
    crs::set_catalog();
    let (list, total) = crs_pick::systems_that_fit(Some(3115000.0), Some(10070000.0), "ft", 60.0).unwrap();
    assert!(total > 0 && list.iter().any(|f| f.entry.code == 2277.0));
    assert_eq!(crs_pick::systems_that_fit(Some(1e12), Some(1e12), "ft", 60.0).unwrap().1, 0);
    assert_eq!(crs_pick::systems_that_fit(None, Some(1.0), "ft", 60.0).unwrap().1, 0);
}

/// A one-subgrid NTv2 file: 1″ north and 2″ west everywhere over [-100, 30]–[-96, 34].
fn ntv2() -> Vec<u8> {
    let (w, s, e, n, step) = (-100.0f64, 30.0f64, -96.0f64, 34.0f64, 1.0f64);
    let cols = ((e - w) / step) as usize + 1;
    let rows = ((n - s) / step) as usize + 1;
    let mut b = vec![0u8; 352 + cols * rows * 16];
    let put = |b: &mut Vec<u8>, o: usize, k: &str, v: &[u8]| {
        b[o..o + 8].copy_from_slice(format!("{k:<8}").as_bytes());
        b[o + 8..o + 8 + v.len()].copy_from_slice(v);
    };
    put(&mut b, 0, "NUM_OREC", &11i32.to_le_bytes());
    put(&mut b, 16, "NUM_SREC", &11i32.to_le_bytes());
    put(&mut b, 32, "NUM_FILE", &1i32.to_le_bytes());
    put(&mut b, 48, "GS_TYPE", b"SECONDS ");
    for (o, k, v) in [
        (176 + 64, "S_LAT", s),
        (176 + 80, "N_LAT", n),
        (176 + 96, "E_LONG", -e),
        (176 + 112, "W_LONG", -w),
        (176 + 128, "LAT_INC", step),
        (176 + 144, "LONG_INC", step),
    ] {
        put(&mut b, o, k, &(v * 3600.0).to_le_bytes());
    }
    put(&mut b, 176 + 160, "GS_COUNT", &((cols * rows) as i32).to_le_bytes());
    for k in 0..cols * rows {
        let p = 352 + k * 16;
        b[p..p + 4].copy_from_slice(&1.0f32.to_le_bytes());
        b[p + 4..p + 8].copy_from_slice(&2.0f32.to_le_bytes());
    }
    b
}

#[test]
fn grid_shift_files() {
    crs::set_catalog();
    let info = crs::load_grid("test-grid.gsb", &ntv2()).unwrap();
    assert_eq!(info.kind, "ntv2");
    assert_eq!(info.subgrids, 1);
    assert!((info.extent[0] + 100.0).abs() < 1e-9 && (info.extent[3] - 34.0).abs() < 1e-9);
    let c = json!({"code": 32040, "name": "NAD27 / Texas South Central", "proj4": "+proj=longlat +datum=NAD27 +no_defs", "unit": "deg", "grid": "test-grid.gsb"});
    assert_eq!(crs::def_for(&c).unwrap(), "+proj=longlat +ellps=clrk66 +no_defs +nadgrids=@test-grid.gsb");
    let st = crs::grid_status(&c, Some(-98.0), Some(32.0));
    assert_eq!(st["inside"], true);
    let p = crs::nn(crs::transform(&c, &Value::Null, -98.0, 32.0).unwrap());
    // the grid moves the point 1″ north and 2″ west (longitudes positive west in NTv2)
    assert!((p[1] - (32.0 + 1.0 / 3600.0)).abs() < 1e-9, "{p:?}");
    assert!(crs::load_grid("", &ntv2()).is_err());
    assert!(crs::load_grid("x", b"not a grid").is_err());
    assert!(crs::load_grid("cut.gsb", &ntv2()[..200]).is_err() || crs::grid_info("cut.gsb").is_some());
}
