//! Fuzz the SketchUp reader: any bytes must give a model or an `SkpError`, never a panic, hang or OOM.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(skp) = cc_io_skp::read_skp(data) {
        // touch the whole model so nothing is optimised away
        let mut n = skp.counts.faces + skp.notes.len();
        for d in &skp.defs {
            for f in &d.faces {
                n += f.loops.iter().map(Vec::len).sum::<usize>();
            }
        }
        std::hint::black_box(n);
    }
});
