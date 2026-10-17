//! Fuzz PartCraft's expression language (expr.js: what people type in a number field, and every parameter and
//! equation of a file): any text must parse or be refused, evaluate or say why, solve backward or give up — never a
//! panic, unbounded recursion or a hang.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let src = String::from_utf8_lossy(data);
    let values = vec![("w".to_string(), 12.5), ("h".to_string(), -3.0), ("x".to_string(), 0.0)];
    let names: Vec<String> = values.iter().map(|(k, _)| k.clone()).collect();
    let _ = cc_part::expr::parse(&src, Some(&names));
    if let Ok(p) = cc_part::expr::parse(&src, None) {
        let _ = cc_part::expr::evaluate(&p.ast, &values);
        let _ = cc_part::expr::solve_for(&p.ast, &values, "w", 7.0, 1e-9);
    }
    let _ = cc_part::expr::literal(&src);
    let _ = cc_part::expr::rename_in(&src, "w", "width");
    let mut p = cc_part::params::Params::default();
    let _ = cc_part::params::set_var(&mut p, "a", &src, None, None);
    let _ = cc_part::params::set_var(&mut p, "b", "a * 2", None, None);
    let _ = cc_part::params::evaluate_vars(&p, None);
    let _ = cc_part::params::drive_var(&mut p, "b", 3.0, None);
    let _ = cc_part::params::read_design_table(&src, &p);
});
