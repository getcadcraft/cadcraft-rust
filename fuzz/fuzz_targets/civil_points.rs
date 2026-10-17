//! Fuzz the survey point file reader (CSV / TXT, every column order), then a surface from what it read.
#![no_main]

use libfuzzer_sys::fuzz_target;

const FORMATS: [&str; 4] = ["", "PNEZD", "PENZD", "NEZ"];

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let fmt = FORMATS.get(data.len() % FORMATS.len()).copied().unwrap_or("");
    let parsed = cc_civil::points::parse_points(&text, fmt, "");
    let pts: Vec<cc_civil::tin::ZPt> = parsed.points.iter().take(5000).map(|p| cc_civil::tin::ZPt { x: p.x, y: p.y, z: p.z }).collect();
    let _ = cc_civil::tin::build_surface(&pts, &[], None, 0.0);
});
