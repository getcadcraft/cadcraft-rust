//! Fuzz MarkupCraft's BCF import (`readBcf`: the zip, the regular-expression XML reader, entities and CDATA) and the
//! import plan built from it. Any bytes must give topics or a refusal, never a panic, hang or OOM.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(bcf) = cc_markup::bcfread::read_bcf(data) {
        let sheets = vec![cc_markup::bcfread::PlanSheet {
            key: "B:0".into(),
            title: "A-101".into(),
            sheet_no: "A-101".into(),
            file: "set.pdf".into(),
            view: [0.0, 0.0, 1296.0, 1728.0],
        }];
        let plan = cc_markup::bcfread::plan_import(&bcf, &[], &[], &[], &sheets, "B:0", "Fuzz", cc_clock::Zone::Utc);
        std::hint::black_box(plan.to_string().len());
    }
});
