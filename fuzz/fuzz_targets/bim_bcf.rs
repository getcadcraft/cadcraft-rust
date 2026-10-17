//! Fuzz BimCraft's BCF reader (zip directory, raw inflate, the small XML reader): any bytes must give topics or an
//! error text, never a panic, hang or OOM. The XML reader also gets the bytes as text directly.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(r) = cc_bim::bcf::read_bcf(data) {
        std::hint::black_box(r.json());
    }
    let text = cc_bim::bcf::decode(data);
    std::hint::black_box(cc_bim::xml::parse_xml(&text).map(|d| d.nodes.len()));
});
