//! Fuzz MarkupCraft's JSON readers for data from outside: a tool chest file (`parseChest`) and a clipboard from
//! another tab (`cleanClip`), then pasting what survives. Any text must give tools / a clip or a refusal, never a
//! panic, hang or OOM.
#![no_main]

use cc_markup::clip::{PlaceOpts, clean_clip_text, place_clip};
use cc_markup::model::Env;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let mut env = Env { now_ms: 1_791_882_000_000.0, count: 0, ..Env::default() };
    if let Ok((tools, skipped)) = cc_markup::chest::parse_chest(&env, &text) {
        std::hint::black_box((tools.len(), skipped));
    }
    if let Some(clip) = clean_clip_text(&mut env, &text) {
        let placed = place_clip(
            &mut env,
            &clip,
            &PlaceOpts { sheet: "f1:0".into(), author: "Fuzz".into(), center: Some([100.0, 100.0]), ..PlaceOpts::default() },
        );
        for m in &placed {
            std::hint::black_box(cc_markup::shapes::shapes(m, &cc_markup::shapes::ShapeEnv::default()).len());
        }
    }
});
