//! Fuzz DetailCraft's strict DSTV NC1 reader (`readNc1`): any bytes as text → { header, blocks } or a refusal, never a
//! panic, hang or OOM.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = cc_io_detail::nc1::read_nc1(&text);
});
