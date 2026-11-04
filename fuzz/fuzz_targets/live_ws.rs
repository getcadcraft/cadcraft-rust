//! Fuzz Studio Live's WebSocket reading (cc_live::ws, M16.9): an upgrade head and the messages after it come from any
//! machine on the LAN. Any bytes must read as a head / message or be refused, within the caps, without panicking.
#![no_main]

use cc_live::ws::{read_head, read_msg};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = read_head(&mut &data[..]);
    for masked in [true, false] {
        let mut r = data;
        for _ in 0..16 {
            if read_msg(&mut r, 64 * 1024, masked).is_err() {
                break;
            }
        }
    }
});
