//! Fuzz PartCraft's part file reader (part.js `readPart`: a `.partcraft.json` is user data) and what reads the part it
//! keeps: any bytes must read or be refused; a part that reads writes back (toJSON) and reads again, lists its
//! drivable numbers, applies its parameters (which re-solves sketches), and every sketch solves and gives a profile —
//! never a panic, a hang or an allocation past the solver's caps.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let raw = match serde_json::from_slice::<serde_json::Value>(data) {
        Ok(v) => v,
        Err(_) => serde_json::Value::String(String::from_utf8_lossy(data).into_owned()),
    };
    let Ok(mut part) = cc_part::part::read_part(&raw) else { return };
    let text = cc_part::part::to_json(&part);
    let _ = cc_part::part::read_part(&serde_json::Value::String(text));
    let _ = cc_part::params_part::targets(&part);
    let _ = cc_part::params_part::params_report(&part);
    let _ = cc_part::part::file_version(&part);
    for f in cc_part::part::features(&part) {
        let _ = cc_part::part::depends_on(f);
    }
    let _ = cc_part::params_part::apply_part_params(&mut part, None);
    let n = cc_part::part::features(&part).len();
    for i in 0..n {
        let Some(sk) = part.get_mut("features").and_then(|f| f.get_mut(i)).and_then(|f| f.get_mut("sketch")).filter(|s| s.is_object())
        else {
            continue;
        };
        // keep a fuzz case quick: the solver's cost grows with the cube of a fit spline's points
        if cc_part::sketch::entities(sk).len() > 200 {
            continue;
        }
        let _ = cc_part::solver::solve_sketch(sk, None, None);
        let _ = cc_part::sketch::profile(sk, 16.0);
    }
    let _ = cc_part::part::delete_features(&mut part, &["f1".to_string()]);
});
