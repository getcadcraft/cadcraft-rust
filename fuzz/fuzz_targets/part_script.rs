//! Fuzz PartCraft scripts (`.pcscript`, script.js): any text must build a part or be refused with line-numbered
//! errors, and any part read back must write again and read again — never a panic, unbounded work or a hang.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let src = String::from_utf8_lossy(data);
    let _ = cc_part::pcscript::looks_like_script(&src);
    let read_file = |p: &str| -> Result<String, String> { if p.len() < 64 { Ok(String::from("ISO-10303-21;")) } else { Err(String::from("no")) } };
    if let Ok(r) = cc_part::pcscript::script_to_part(&src, Some(&read_file)) {
        let (text, _) = cc_part::pcscript::part_to_script(&r.part, false, false, true);
        let _ = cc_part::pcscript::script_to_part(&text, Some(&read_file));
        let _ = cc_part::pcscript::part_to_script(&r.part, true, true, false);
    }
    let _ = cc_part::v8json::parse_error(&src);
});
