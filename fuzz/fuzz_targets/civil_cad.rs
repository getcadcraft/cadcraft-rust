//! Fuzz CivilCraft's DWG / DXF linework (gis-cad-ui.js: cad-linework.js and block-points.js over cc-io-dwg's DXF model):
//! any text gives layers, block points or nothing, never a panic, hang or OOM.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = cc_io_dwg::dxf_model::decode_dxf_bytes(data);
    let Ok(model) = cc_io_dwg::dxf_model::try_read_dxf_model(&text) else { return };
    let r = cc_io_civil::cad_linework::cad_linework(&model, "f.dxf", "ft", None, None);
    let br = cc_io_civil::block_points::block_refs(&model, "m", Some(r.scale));
    let blocks: Vec<String> = br.blocks.iter().map(|b| b.0.clone()).collect();
    let o = cc_io_civil::block_points::FromBlocks {
        blocks: &blocks,
        layers: &[],
        desc: "attr:DESC",
        z: "attr:ELEV",
        number: "attr:PT",
        existing: &["1".to_string()],
        start: None,
    };
    let _ = cc_io_civil::block_points::points_from_blocks(&br.refs, &o);
});
