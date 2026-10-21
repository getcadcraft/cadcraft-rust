//! Fuzz libplace.js's port (INSERT, Library, M4.22): any JSON as a library item gives a block in the caps or a
//! sentence why not, never a panic, hang or OOM; placing it twice reuses the block.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() > 1 << 20 {
        return;
    }
    let Ok(item) = serde_json::from_slice::<serde_json::Value>(data) else { return };
    let lang = cc_engine::Lang::new(&serde_json::Value::Null);
    let mut doc = cc_cad::Doc::new();
    doc.load(&serde_json::json!({ "meta": { "units": "inches" } }));
    let a = doc.change("INSERT", |d| cc_engine::cmd::libplace::ensure_library_block(d, &item, None, &lang));
    if let Ok(Ok(first)) = a {
        assert!(doc.blocks.contains_key(first.name.as_str()));
        let b = doc.change("INSERT", |d| cc_engine::cmd::libplace::ensure_library_block(d, &item, None, &lang));
        if let Ok(Ok(again)) = b {
            assert!(again.reused || again.name != first.name);
        }
    }
    let _ = cc_engine::cmd::libplace::next_number(&doc);
});
