//! Fuzz DetailCraft's IFC import (`importIfc` through cc-ifc): any bytes → a model and a report, or a refusal; never a
//! panic, hang or OOM.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(r) = cc_io_detail::ifc_import::import_ifc(data, "fuzz") {
        let _ = cc_io_detail::ifc_import::report_lines(r.get("report").unwrap_or(&serde_json::Value::Null));
    }
});
