//! Fuzz CivilCraft's GeoPackage reader (its own SQLite reader): any bytes → open_gpkg → read every table, and
//! parse_gpb / read_wkb on the raw bytes, must give data or an error, never a panic, hang or OOM. The tables read are
//! written again and opened back.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = cc_io_civil::gpkg::parse_gpb(data);
    let _ = cc_io_civil::gis_geom::read_wkb(data, 0);
    let Ok(gp) = cc_io_civil::gpkg::open_gpkg(data) else { return };
    let mut layers = Vec::new();
    for t in &gp.tables {
        if let Ok(ls) = gp.read(&t.name, 10_000) {
            layers.extend(ls.into_iter().map(|l| (t.name.clone(), l)));
        }
    }
    let groups: Vec<cc_io_civil::gpkg::GGroup> = layers
        .iter()
        .map(|(n, l)| cc_io_civil::gpkg::GGroup { table: n, label: n, geom: &l.geom, has_z: l.has_z, fields: &l.fields, features: &l.features })
        .collect();
    if let Ok(out) = cc_io_civil::gpkg::write_gpkg(&groups, None)
        && let Ok(back) = cc_io_civil::gpkg::open_gpkg(&out)
    {
        for t in &back.tables {
            let _ = back.read(&t.name, 10_000);
        }
    }
});
