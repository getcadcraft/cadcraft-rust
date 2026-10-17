//! Fuzz CivilCraft's KML reader: any text → read_kml must give layers or the JS error message, never a panic, hang
//! or OOM; what it reads is written again and read back.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    if let Ok((layers, _)) = cc_io_civil::kml::read_kml(&text, "f", 10_000) {
        let groups: Vec<cc_io_civil::kml::KGroup> = layers.iter().map(|l| cc_io_civil::kml::KGroup { kind: &l.name, has_z: l.has_z, features: &l.features }).collect();
        let out = cc_io_civil::kml::write_kml(&groups, &|x, y| (x, y), &|z| z, "f");
        let _ = cc_io_civil::kml::read_kml(&out, "b", 10_000);
    }
});
