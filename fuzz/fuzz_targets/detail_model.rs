//! Fuzz DetailCraft's model file: any bytes → `readModel` (the untrusted-file checks) → build (connections, parts,
//! stairs, footings) → marks, bill of materials, bending schedule → saved text read back. A result or an error message,
//! never a panic, hang or OOM.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data).into_owned();
    let mut m = cc_detail::model::Model::default();
    if m.load(&serde_json::Value::String(text)).is_err() {
        return;
    }
    if let Ok(b) = cc_detail::build::build(&m.state) {
        let k = cc_detail::marks::mark_parts(&b, 1.0, true);
        let _ = cc_detail::marks::bom_tables(&k, &b.bolts);
        let fs: Vec<cc_detail::rebar::Footing> = b.footings.iter().map(|f| f.f.clone()).collect();
        let s = cc_detail::rebar::bending_schedule(&fs);
        let _ = cc_detail::rebar::schedule_table(&s.rows, "bs8666", "metric");
    }
    let saved = m.to_json();
    let _ = cc_detail::model::read_model(&serde_json::Value::String(saved));
});
