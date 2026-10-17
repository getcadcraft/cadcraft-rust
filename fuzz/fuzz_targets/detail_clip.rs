//! Fuzz DetailCraft's clipboard as read back from storage (untrusted: another tab, an old version, a hand edit): any
//! bytes → `readClip` → paste into a model (one change) → build and clash check. A result or an error message, never a
//! panic, hang or OOM.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data).into_owned();
    let Ok(clip) = cc_detail::clipboard::read_clip(&serde_json::Value::String(text)) else { return };
    let _ = cc_detail::clipboard::clip_segments(&clip);
    let mut m = cc_detail::model::Model::default();
    let off = serde_json::json!([100, 0, 0]);
    if m.change("Paste", |tx| cc_detail::clipboard::paste_into(tx, &clip, Some(&off))).is_ok() {
        if let Ok(b) = cc_detail::build::build(&m.state) {
            let _ = cc_detail::clash::clash_check(&m.state, &b, 1.0, None);
        }
    }
});
