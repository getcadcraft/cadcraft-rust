//! Fuzz ModelCraft's terrain (cc-m3d-tools terrain grids) and the solid checker over it: grids of any size and
//! spacing, sculpted corners, id counters near and past the kernel's dense id range, faces added and deleted, then
//! the checker's problems, info and Fix and a re-face of the ground plane. Nothing may panic, hang or run out of
//! memory (the driver is cc_m3d_tools::fuzzterrain, which the crate's tests also run; M6.36).
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    cc_m3d_tools::fuzzterrain::run(data);
});
