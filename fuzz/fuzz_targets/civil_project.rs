//! Fuzz CivilCraft's project file: any bytes → JSON → Project::from_json → eg / graded / volumes / to_json, and the
//! file read back. A result or an error message, never a panic, hang or OOM.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(data) else { return };
    if let Ok((mut p, _notes)) = cc_civil::project::Project::from_json(&v) {
        let _ = p.eg();
        let _ = p.graded();
        let _ = p.network_check();
        let j = p.to_json();
        let _ = cc_civil::project::Project::from_json(&j);
    }
});
