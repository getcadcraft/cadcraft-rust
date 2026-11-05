//! Fuzz DetailCraft's state held as records (S.16) against the model before it (one JSON value, `reference`): the
//! input's first part is a file's text, read both ways (the same state text and warnings, or the same error); the rest
//! picks edits (every Tx method with arguments of every kind, undo, redo) made both ways on what was read (or the
//! sample): the same results, errors and state text after each. Neither may panic, hang or run out of memory.
#![no_main]

use libfuzzer_sys::fuzz_target;
use serde_json::Value;

fuzz_target!(|data: &[u8]| {
    let cut = data.iter().position(|b| *b == 0).unwrap_or(data.len());
    let (text, picks) = data.split_at(cut);
    let text = String::from_utf8_lossy(text).into_owned();
    if let Err(e) = cc_detail::reference::check_read(&text) {
        panic!("{e}");
    }
    let start = match cc_detail::reference::read_model(&Value::String(text)) {
        Ok((s, _)) => s,
        Err(_) => match cc_detail::sample::sample_model("Sample frame") {
            Ok(m) => m.state.to_value(),
            Err(_) => return,
        },
    };
    let picks = picks.get(1..).unwrap_or(&[]);
    if picks.is_empty() {
        return;
    }
    let mut p = cc_detail::reference::Picks::bytes(picks);
    if let Err(e) = cc_detail::reference::check_edits(&start, &mut p, 64) {
        panic!("{e}");
    }
});
