//! Fuzz CivilCraft's shapefile reader: any bytes split into .shp and .dbf (the .shp length from the first two bytes) →
//! read_shapefile (and read_dbf alone, every code page) must give a layer or the JS error message, never a panic,
//! hang or OOM; what it reads is written again and read back.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((head, rest)) = data.split_first_chunk::<2>() else { return };
    let (shp, dbf) = rest.split_at(usize::from(u16::from_le_bytes(*head)).min(rest.len()));
    for cpg in [None, Some("UTF-8"), Some("1252"), Some("SJIS"), Some("garbage")] {
        let _ = cc_io_civil::dbf::read_dbf(dbf, cpg, 10_000);
    }
    let set = cc_io_civil::shapefile::ShpSet { name: "f".into(), shp: Some(shp.to_vec()), shx: None, dbf: Some(dbf.to_vec()), prj: None, cpg: None };
    if let Ok(l) = cc_io_civil::shapefile::read_shapefile(&set, 10_000) {
        let out = cc_io_civil::shapefile::write_shapefile(&l.geom, l.has_z, &l.fields, &l.features, (2024, 5, 17));
        let back = cc_io_civil::shapefile::ShpSet { name: "b".into(), shp: Some(out.shp), shx: Some(out.shx), dbf: Some(out.dbf), prj: None, cpg: Some(b"UTF-8".to_vec()) };
        let _ = cc_io_civil::shapefile::read_shapefile(&back, 10_000);
    }
});
