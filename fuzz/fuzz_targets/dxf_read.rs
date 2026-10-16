//! Fuzz the DXF reader and the 2D compile step: any bytes must give a model (or the JS error message) and a
//! drawing, never a panic, hang or OOM.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = cc_io_dwg::dxf_model::decode_dxf_bytes(data);
    if let Ok(model) = cc_io_dwg::dxf_model::try_read_dxf_model(&text) {
        let drawing = cc_io_dwg::compile::compile_drawing(&model, "", &[]);
        let _ = cc_io_dwg::compile::to_json(&drawing);
        let _ = cc_io_dwg::V::Obj(model).to_json();
    }
});
