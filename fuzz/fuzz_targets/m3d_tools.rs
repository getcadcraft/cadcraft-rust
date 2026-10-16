//! Fuzz ModelCraft's tools (cc-m3d-tools): random event sequences — tool choices, pointer moves, clicks with
//! modifiers, drags, keys, typed text, selections, open contexts, camera moves, flags, undo / redo, tool methods,
//! picking queries and outside edits — into the Tools over a small model. Nothing may panic, hang or run out of
//! memory (the driver is cc_m3d_tools::fuzzdrive, which the crate's tests also run).
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    cc_m3d_tools::fuzzdrive::run(data);
});
