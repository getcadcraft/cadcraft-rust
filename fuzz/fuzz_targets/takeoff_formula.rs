//! Fuzz TakeoffCraft's formula parser and evaluator (formula.js; formulas come from project files, price libraries,
//! trade packs and the clipboard): any text must parse or be refused with a message and a position, and a parsed tree
//! must evaluate to a finite number or an error, never panic, recurse past its depth limit or hang.
#![no_main]

use cc_takeoff::formula;
use libfuzzer_sys::fuzz_target;

const VARS: [&str; 6] = ["L", "A", "P", "N", "H", "V"];

fuzz_target!(|data: &[u8]| {
    let Ok(src) = std::str::from_utf8(data) else { return };
    match formula::parse(src, &VARS) {
        Ok(p) => {
            for vals in [[12.5, 135.0, 54.0, 3.0, 9.0, 1.0], [0.0; 6], [-1e300, 1e300, f64::MIN_POSITIVE, 1e15, -0.0, 7.0]] {
                let get = |n: &str| VARS.iter().position(|v| *v == n).and_then(|i| vals.get(i).copied());
                let _ = formula::evaluate(&p.ast, &get);
            }
        }
        Err(e) => {
            let _ = (e.message.len(), e.pos);
        }
    }
    let _ = formula::compute(src, &[("L".to_string(), 10.0), ("N".to_string(), 2.0)]);
    let _ = formula::parse(src, &[]);
});
