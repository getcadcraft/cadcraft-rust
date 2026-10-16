//! Fuzz the DWG reader: any bytes must give a model (with notes), never a panic, hang or OOM.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let model = cc_io_dwg::read_dwg(data, &cc_io_dwg::ReadOpts::default());
    let _ = cc_io_dwg::V::Obj(model).to_json();
});
