//! Fuzz DetailCraft's typed model read (S.15) against the JSON read: any text → `readModel` on the text (members and
//! connections read straight into their fields) and `readModel` on the text parsed as JSON must give the same state
//! and warnings, or the same error. Neither may panic, hang or run out of memory.
#![no_main]

use cc_detail::model::read_model;
use libfuzzer_sys::fuzz_target;
use serde_json::Value;

fn show(r: Result<(Value, Vec<String>), String>) -> String {
    match r {
        Ok((s, w)) => format!("ok {} {}", cc_detail::js::stringify(&s), w.join("|")),
        Err(e) => format!("err {e}"),
    }
}

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data).into_owned();
    let typed = show(read_model(&Value::String(text.clone())));
    let json = show(cc_detail::model::read_model_json(&text));
    assert_eq!(typed, json);
});
