//! Fuzz the drafting app's DXF importer and its writers: any bytes → read_dxf → Doc::load → write_dxf (R12, and the
//! written text read back), write_dxf2000 and save_dwg (one DWG version, picked from the input's length) must give a
//! result or the JS error message, never a panic, hang or OOM.
#![no_main]

use libfuzzer_sys::fuzz_target;

const VERSIONS: [&str; 6] = ["2000", "2004", "2007", "2010", "2013", "2018"];

fuzz_target!(|data: &[u8]| {
    let text = cc_io_dwg::dxf_model::decode_dxf_bytes(data);
    if let Ok(input) = cc_io_cad::dxf_read::read_dxf(&text) {
        let mut doc = cc_cad::Doc::new();
        doc.load(&input);
        if let Ok((out, _report)) = cc_io_cad::dxf_write::write_dxf(&doc) {
            let _ = cc_io_cad::dxf_read::read_dxf(&out);
        }
        if let Ok((out, _report)) = cc_io_cad::dxf2000::write_dxf2000(&doc) {
            let _ = cc_io_cad::dxf_read::read_dxf(&out);
        }
        let version = VERSIONS.get(data.len() % VERSIONS.len()).copied().unwrap_or("2000");
        let opts =
            cc_io_cad::dwg_save::SaveOpts { version: version.into(), created_ms: Some(0.0), guid_sequence: true, ..Default::default() };
        let _ = cc_io_cad::dwg_save::save_dwg(&doc, &opts);
    }
});
