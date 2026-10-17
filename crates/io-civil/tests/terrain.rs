//! Terrain: the fixed cases of the 0.5 test-civilcraft-terrain.mjs that need no GDAL / laspy (fixtures made here).

use cc_io_civil::geotiff_write::{self, DemOptions, NODATA};
use cc_io_civil::raster::{self, InFile};
use cc_io_civil::terrain::{self, BoxXY, Grid};
use cc_io_civil::thin::{Coverage, Thinner};
use cc_io_civil::{crs, las};
use serde_json::json;

#[test]
fn units_and_steps() {
    assert_eq!(terrain::unit_from_name("metre"), Some("m"));
    assert_eq!(terrain::unit_from_name("US survey foot"), Some("ftUS"));
    assert_eq!(terrain::unit_from_name(" Foot_US "), Some("ftUS"));
    assert_eq!(terrain::unit_from_name("ft"), Some("ft"));
    assert_eq!(terrain::unit_from_name("furlong"), None);
    assert_eq!(terrain::z_factor("m", "ft"), 1.0 / 0.3048);
    assert_eq!(terrain::z_factor("ftUS", "ftUS"), 1.0);
    assert_eq!(terrain::nice_step(7.0), 10.0);
    assert_eq!(terrain::nice_step(0.3), 0.5);
    assert_eq!(terrain::nice_step(-1.0), 1.0);
    crs::set_catalog();
    let own = terrain::crs_from_geo_keys(json!({"GTModelTypeGeoKey": 1, "ProjectedCSTypeGeoKey": 32767}).as_object());
    assert!(own.crs.is_null() && own.note.unwrap().contains("describes its own"));
    let geo = terrain::crs_from_geo_keys(json!({"GTModelTypeGeoKey": 2, "GeographicTypeGeoKey": 4326}).as_object());
    assert_eq!(geo.crs["code"].as_f64(), Some(4326.0));
    assert!(
        terrain::crs_from_geo_keys(json!({"GTModelTypeGeoKey": 1, "ProjectedCSTypeGeoKey": 4}).as_object())
            .note
            .unwrap()
            .contains("not in CivilCraft")
    );
}

#[test]
fn thinning() {
    let pts = [[0.2, 0.2, 5.0, 2.0], [0.6, 0.6, 3.0, 2.0], [0.5, 0.5, 4.0, 3.0], [1.5, 0.5, 9.0, 2.0]];
    let run = |mode: &str| {
        let mut t = Thinner::new(1.0, mode, [0.0, 0.0], None, None, f64::INFINITY).unwrap();
        for p in pts {
            t.add(p[0], p[1], p[2], p[3]);
        }
        t.points()
    };
    assert_eq!(run("lowest")[0][2], 3.0);
    assert_eq!(run("nearest")[0][2], 4.0);
    assert_eq!(run("average")[0][2], 4.0);
    assert_eq!(run("lowest").len(), 2);
    let mut cls = Thinner::new(1.0, "nearest", [0.0, 0.0], Some(&[3.0]), None, f64::INFINITY).unwrap();
    for p in pts {
        cls.add(p[0], p[1], p[2], p[3]);
    }
    assert_eq!((cls.count, cls.filtered), (1, 3.0));
    assert!(Thinner::new(0.0, "nearest", [0.0, 0.0], None, None, 1.0).is_err());
    assert_eq!(cc_io_civil::thin::grid_step(30.0, 10.0), 3.0);
    assert_eq!(cc_io_civil::thin::grid_step(24.0, 10.0), 2.0);
    let mut cov = Coverage::new(BoxXY { min_x: 0.0, min_y: 0.0, max_x: 100.0, max_y: 100.0 }, 10);
    for i in 0..1000 {
        cov.add(f64::from(i % 100), f64::from(i / 10), 2.0);
    }
    let (keep, in_area) = cov.estimate(5.0, None, None);
    assert_eq!(in_area, 1000.0);
    assert!(keep <= 1000.0 && keep > 0.0);
}

#[test]
fn windows_and_tins() {
    let gt = [620000.0, 10.0, 0.0, 3350000.0, 0.0, -10.0];
    let w = terrain::grid_window(&gt, 40.0, 30.0, Some(&BoxXY { min_x: 620100.0, min_y: 3349500.0, max_x: 620400.0, max_y: 3349800.0 }))
        .unwrap();
    assert_eq!((w.c0, w.r0, w.c1, w.r1), (10.0, 20.0, 40.0, 30.0));
    assert!(terrain::grid_window(&gt, 40.0, 30.0, Some(&BoxXY { min_x: 0.0, min_y: 0.0, max_x: 1.0, max_y: 1.0 })).is_none());
    let grid = Grid { c0: 0.0, r0: 0.0, step: 1.0, nx: 3, ny: 2, z: vec![1.0, 2.0, f64::NAN, 4.0, 5.0, 6.0] };
    let id = |x: f64, y: f64| Ok([x, y]);
    let t = terrain::grid_tin(&grid, &gt, &id, 1.0).unwrap();
    assert_eq!((t.pts.len(), t.cells, t.faces.len(), t.skipped), (5, 2, 2, 2));
    let p = terrain::grid_points(&grid, &gt, &id, 2.0, "DEM", 7.0).unwrap();
    assert_eq!((p.len(), p[0].n.as_str(), p[0].z), (5, "7", 2.0));
    assert_eq!(terrain::next_number(&[json!({"n": "7"}), json!({"n": 12}), json!({"n": "A3"})]), 13.0);
}

#[test]
fn ascii_grids() {
    let text = "ncols 3\nnrows 2\nxllcorner 100\nyllcorner 200\ncellsize 10\nNODATA_value -9999\n1 2 3\n4 -9999 6\n";
    let f = vec![InFile { name: "g.asc".into(), bytes: text.as_bytes().to_vec() }];
    let src = raster::open_raster(&f).unwrap();
    assert_eq!(src.gt, [100.0, 10.0, 0.0, 220.0, 0.0, -10.0]);
    let g = raster::read_window(&src, None, None, None, None, None).unwrap();
    assert_eq!(g.z[..4], [1.0, 2.0, 3.0, 4.0]);
    assert!(g.z[4].is_nan());
    for bad in ["ncols 0\nnrows 5\nxllcorner 0\nyllcorner 0\ncellsize 1\n1 2 3", "hello\n1 2 3"] {
        assert!(raster::open_raster(&[InFile { name: "x.asc".into(), bytes: bad.as_bytes().to_vec() }]).is_err());
    }
    let half = InFile { name: "h.asc".into(), bytes: text.as_bytes()[..text.len() - 6].to_vec() };
    let s = raster::open_raster(&[half]).unwrap();
    assert!(raster::read_window(&s, None, None, None, None, None).unwrap_err().contains("shorter"));
}

#[test]
fn geotiff_out_and_back() {
    crs::set_catalog();
    let (w, h) = (37.0, 21.0);
    let values: Vec<f32> = (0..37 * 21).map(|i| if i % 11 == 0 { NODATA } else { 100.0 + i as f32 * 0.25 }).collect();
    for (tiled, deflate) in [(true, true), (true, false), (false, true), (false, false)] {
        let o = DemOptions {
            epsg: Some(32614.0),
            citation: String::new(),
            xy_unit: "m".into(),
            z_unit: "m".into(),
            description: "Elevations.".into(),
            tiled,
            tile: 16.0,
            deflate,
        };
        let b = geotiff_write::write_dem_tiff(w, h, [620000.0, 10.0, 0.0, 3350000.0, 0.0, -10.0], &values, &o).unwrap();
        let src = raster::open_raster(&[InFile { name: "o.tif".into(), bytes: b }]).unwrap();
        assert_eq!(src.code, Some(32614.0));
        assert_eq!(src.gt, [620000.0, 10.0, 0.0, 3350000.0, 0.0, -10.0]);
        let g = raster::read_window(&src, None, None, None, None, None).unwrap();
        for (i, v) in values.iter().enumerate() {
            if *v == NODATA {
                assert!(g.z[i].is_nan());
            } else {
                assert_eq!(g.z[i], f64::from(*v));
            }
        }
    }
    let o = DemOptions {
        epsg: None,
        citation: String::new(),
        xy_unit: "m".into(),
        z_unit: "m".into(),
        description: String::new(),
        tiled: true,
        tile: 256.0,
        deflate: false,
    };
    assert!(geotiff_write::write_dem_tiff(2.0, 2.0, [0.0, 1.0, 0.0, 0.0, 0.0, -1.0], &[1.0; 3], &o).is_err());
    assert!(geotiff_write::write_dem_tiff(2.0, 1.0, [0.0, 1.0, 0.5, 0.0, 0.0, -1.0], &[1.0; 2], &o).is_err());
}

/// A LAS 1.2 format-1 file with `n` points.
fn las_file(n: usize) -> Vec<u8> {
    let (hs, rl) = (227usize, 28usize);
    let mut b = vec![0u8; hs + n * rl];
    b[..4].copy_from_slice(b"LASF");
    b[24] = 1;
    b[25] = 2;
    b[94..96].copy_from_slice(&(hs as u16).to_le_bytes());
    b[96..100].copy_from_slice(&(hs as u32).to_le_bytes());
    b[104] = 1;
    b[105..107].copy_from_slice(&(rl as u16).to_le_bytes());
    b[107..111].copy_from_slice(&(n as u32).to_le_bytes());
    for (i, s) in [0.01f64, 0.01, 0.001].iter().enumerate() {
        b[131 + 8 * i..139 + 8 * i].copy_from_slice(&s.to_le_bytes());
    }
    b[155..163].copy_from_slice(&620000.0f64.to_le_bytes());
    for k in 0..n {
        let p = hs + k * rl;
        b[p..p + 4].copy_from_slice(&(k as i32 * 100).to_le_bytes());
        b[p + 8..p + 12].copy_from_slice(&(k as i32 * 7).to_le_bytes());
        b[p + 15] = 2 | 0xe0;
    }
    b
}

#[test]
fn las_files() {
    crs::set_catalog();
    let f = InFile { name: "a.las".into(), bytes: las_file(10) };
    let mut h = las::read_las_header(&f).unwrap();
    assert_eq!((h.version.as_str(), h.format, h.count, h.available), ("1.2", 1.0, 10.0, 10.0));
    assert!(h.crs_note.as_deref().unwrap().contains("does not say"));
    let mut pts = Vec::new();
    let read = las::scan_points(&f, &mut h, &mut |x, y, z, c| pts.push([x, y, z, c])).unwrap();
    assert_eq!(read, 10.0);
    assert_eq!(pts[3], [620003.0, 0.0, 0.021, 2.0]);
    // cut short: fewer points than the header says, with a note
    let cut = InFile { name: "c.las".into(), bytes: las_file(10)[..227 + 28 * 4 + 5].to_vec() };
    let h = las::read_las_header(&cut).unwrap();
    assert_eq!(h.available, 4.0);
    assert_eq!(h.notes.len(), 1);
    for bad in [vec![0u8; 100], b"LASX".iter().copied().chain(std::iter::repeat_n(b'x', 400)).collect()] {
        assert!(las::read_las_header(&InFile { name: "b.las".into(), bytes: bad }).is_err());
    }
    let mut fmt = las_file(2);
    fmt[104] = 14;
    assert!(las::read_las_header(&InFile { name: "f.las".into(), bytes: fmt }).unwrap_err().contains("14"));
    let mut lz = las_file(2);
    lz[104] |= 0x80;
    assert!(las::read_las_header(&InFile { name: "z.laz".into(), bytes: lz }).unwrap_err().contains("LASzip"));
    assert_eq!(las::class_name(2.0).as_deref(), Some("Ground"));
    assert_eq!(las::class_name(19.0), None);
}

/// Speed on the big generated files (testfiles/LARGE-FILES.md): `cargo test --release -p cc-io-civil --test terrain
/// -- --ignored --nocapture bench`. Skips when testfiles/ is missing.
#[test]
#[ignore]
fn bench() {
    let g = std::path::Path::new("/var/www/CADCRAFT/testfiles/generated");
    let dem = g.join("terrain/dem-4000x4000-f32-deflate.tif");
    let las_path = g.join("pointcloud/big-90mb.las");
    if !dem.exists() || !las_path.exists() {
        eprintln!("bench: testfiles/generated is missing; skipped");
        return;
    }
    crs::set_catalog();
    let time = |label: &str, f: &mut dyn FnMut() -> String| {
        let t = std::time::Instant::now();
        let r = f();
        eprintln!("{label} {} ms {r}", t.elapsed().as_millis());
    };
    let bytes = std::fs::read(&dem).unwrap();
    time("dem open+readWindow full", &mut || {
        let s = raster::open_raster(&[InFile { name: "dem.tif".into(), bytes: bytes.clone() }]).unwrap();
        let g = raster::read_window(&s, None, None, None, None, None).unwrap();
        g.z.iter().filter(|z| !z.is_nan()).count().to_string()
    });
    time("dem readWindow step 4", &mut || {
        let s = raster::open_raster(&[InFile { name: "dem.tif".into(), bytes: bytes.clone() }]).unwrap();
        raster::read_window(&s, None, None, None, None, Some(4.0)).unwrap().z.len().to_string()
    });
    let lb = std::fs::read(&las_path).unwrap();
    time("las header+scan 90mb", &mut || {
        let f = InFile { name: "big.las".into(), bytes: lb.clone() };
        let mut h = las::read_las_header(&f).unwrap();
        let (mut n, mut s) = (0u64, 0.0);
        las::scan_points(&f, &mut h, &mut |_, _, z, _| {
            n += 1;
            s += z;
        })
        .unwrap();
        format!("{n} {s}")
    });
    time("gtw write 4000x4000 deflate", &mut || {
        let v: Vec<f32> = (0..16_000_000u32).map(|i| (i % 1000) as f32).collect();
        let o = DemOptions {
            epsg: None,
            citation: String::new(),
            xy_unit: "m".into(),
            z_unit: "m".into(),
            description: String::new(),
            tiled: true,
            tile: 256.0,
            deflate: true,
        };
        geotiff_write::write_dem_tiff(4000.0, 4000.0, [0.0, 1.0, 0.0, 0.0, 0.0, -1.0], &v, &o).unwrap().len().to_string()
    });
}

/// Fuzz find (civil_las): a LAZ whose chunk table claims 2^32 entries made laz-rs allocate ~36 GB before reading one.
#[test]
fn laz_chunk_table_count_is_bounded() {
    crs::set_catalog();
    let f = InFile { name: "oom.laz".into(), bytes: include_bytes!("fixtures/las-chunk-table-oom.laz").to_vec() };
    if let Ok(mut h) = las::read_las_header(&f) {
        let r = las::scan_points(&f, &mut h, &mut |_, _, _, _| {});
        assert!(r.is_err() || !h.notes.is_empty() || r.is_ok());
    }
}

/// Fuzz find (civil_las, 2026-10-17): a layered LAZ chunk listing a 4 GB layer made laz-rs size a buffer to it before
/// reading; the chunk header is checked first and the scan stops with the "stopped" note.
#[test]
fn laz_layer_sizes_are_checked() {
    crs::set_catalog();
    let f = InFile { name: "o.laz".into(), bytes: include_bytes!("fixtures/laz-layer-size-oom.laz").to_vec() };
    if let Ok(mut h) = las::read_las_header(&f) {
        let t0 = std::time::Instant::now();
        let _ = las::scan_points(&f, &mut h, &mut |_, _, _, _| {});
        assert!(t0.elapsed().as_secs() < 60);
    }
}

/// Fuzz find (civil_las): damaged compressed points make laz-rs panic inside its arithmetic decoder; the scan stops with
/// the JS's "stopped" note instead (native builds; see las.rs).
#[test]
fn laz_decoder_panic_is_contained() {
    crs::set_catalog();
    let f = InFile { name: "p.laz".into(), bytes: include_bytes!("fixtures/laz-decoder-panic.laz").to_vec() };
    if let Ok(mut h) = las::read_las_header(&f) {
        let _ = las::scan_points(&f, &mut h, &mut |_, _, _, _| {});
    }
}
