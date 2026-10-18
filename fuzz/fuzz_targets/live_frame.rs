//! Fuzz Studio Live's wire reading (cc_live::msgpack, envelope, the session core; frames come from other people's
//! machines and through the relay): any bytes must decode or be refused, an envelope must re-encode, and an edit made of
//! them must be decided without panicking.
#![no_main]

use cc_live::envelope::Envelope;
use cc_live::msgpack::{pack, unpack};
use cc_live::session::{Core, Role};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(v) = unpack(data) {
        let _ = pack(&v);
    }
    if let Ok(env) = Envelope::decode(data) {
        let _ = env.encode();
        let mut core = Core::new();
        let h = core.add("Host", Role::Host);
        let a = core.add("Ana", Role::Editor);
        let _ = core.message(a, &env, 0);
        let _ = core.message(h, &env, 1);
        let _ = core.since(0);
    }
});
