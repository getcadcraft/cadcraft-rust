//! Fuzz what BimCraft reads back from storage as JSON text: saved search sets (ifc-find.js cleanSets), required-
//! property rules (ifc-checks.js cleanRules) and colour schemes (ifc-props.js readScheme). Never a panic.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let sets = cc_bim::find::clean_sets(Some(&text), None);
    std::hint::black_box(sets.iter().map(|s| s.json()).count());
    std::hint::black_box(cc_bim::checks::clean_rules(Some(&text), None));
    if let Ok(s) = cc_bim::props::read_scheme(Some(&text), None) {
        let ix = cc_bim::props::Index::from_json(&serde_json::json!({ "ids": [1, 2, 3], "values": { "cls": [[1, "IfcWall"], [2, 5.5]] }, "fields": [{ "id": "cls" }] }));
        std::hint::black_box(cc_bim::props::apply_scheme(&ix, &s).json());
    }
});
