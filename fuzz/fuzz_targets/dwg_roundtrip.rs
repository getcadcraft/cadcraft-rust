//! Fuzz the DWG writer through the reader: any bytes are read, the model is written as R2018 and as R2000 and both
//! files are read back. Never a panic, hang or OOM (the writer treats a model as hostile input too).
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let model = cc_io_dwg::read_dwg(data, &cc_io_dwg::ReadOpts::default());
    for version in ["AC1032", "AC1015"] {
        let opts = cc_io_dwg::WriteOpts { version: version.into(), created_ms: Some(0.0), guid: cc_io_dwg::GuidMode::Sequence };
        if let Ok((bytes, report)) = cc_io_dwg::write_dwg_opts(&model, &opts) {
            let _ = cc_io_dwg::V::Obj(report).to_json();
            let back = cc_io_dwg::read_dwg(&bytes, &cc_io_dwg::ReadOpts::default());
            let _ = back.get("version");
        }
    }
});
