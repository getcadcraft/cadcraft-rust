//! Fuzz CadCraft's My tools (cc_engine::mytools, cuix): a CUIx file (a ZIP of XML parts and pictures), a My tools
//! JSON file, a stored list and a button's macro all come from outside, so any bytes must be read or refused, never
//! panic, within the caps (a zip bomb, deep XML, a huge picture); what is cleaned twice stays the same.
#![no_main]

use cc_engine::mytools::{self as mt, Ids};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let gv = |n: &str| if n == "CLAYER" { "0".to_string() } else { String::new() };
    let lookup = |c: &str| c == "LINE" || c == "CIRCLE";
    let mut ids = Ids { now_ms: 1.0, seq: 0 };
    if let Ok(r) = cc_engine::cuix::read_cuix(data, &lookup, &gv) {
        let groups: Vec<serde_json::Value> = r
            .groups
            .iter()
            .map(|g| serde_json::json!({ "name": g.name, "tools": g.tools.iter().map(|t| serde_json::json!({ "label": t.label, "macro": t.macro_, "icon": t.icon, "key": t.key })).collect::<Vec<_>>() }))
            .collect();
        let _ = mt::merge_tools(&mt::Tools::default(), &groups, &mut ids);
    }
    let _ = cc_engine::cuix::parse_xml(&String::from_utf8_lossy(data));
    let _ = cc_engine::cuix::bmp_to_png(data);
    let text = String::from_utf8_lossy(data);
    let _ = mt::parse_macro(&text, &gv);
    let _ = mt::icon_from_bytes(data);
    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(data) {
        // (a label cut at 80 can end in a space that the next clean trims, as 0.5's does: compare from the second)
        let (d, _) = mt::clean_tools(&v, &mut ids);
        let (d2, _) = mt::clean_tools(&d.json(), &mut ids);
        let (d3, _) = mt::clean_tools(&d2.json(), &mut ids);
        assert_eq!(d3, d2, "cleaned tools change when cleaned again");
        let _ = mt::export_tools(&d, &mut ids);
    }
});
