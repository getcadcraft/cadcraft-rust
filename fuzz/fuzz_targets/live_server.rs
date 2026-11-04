//! Fuzz Studio Live's session server (cc_live::server, M16.9) and frame opening (cc_live::crypto): a stream of
//! envelopes from a network connection — handshake messages, hellos, edits, host messages — split out of the input,
//! must be decided or refused without panicking; any bytes given to a sealed session's `open` must be refused cleanly.
#![no_main]

use cc_live::envelope::Envelope;
use cc_live::server::{Config, Session};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else { return };
    let cfg = Config { app: "cadcraft".into(), auto_approve: mode & 1 == 1, plain: mode & 2 == 2, local_token: "t".into(), ..Default::default() };
    let Some(mut s) = Session::new(cfg) else { return };
    s.open(1, mode & 4 == 4, "lan", 0);
    s.open(2, false, "lan", 0);
    let mut now = 0u64;
    // length-prefixed envelopes (1 byte length), alternating connections
    let mut r = rest;
    let mut i = 0u64;
    while let Some((&n, tail)) = r.split_first() {
        let n = usize::from(n).min(tail.len());
        let (chunk, more) = tail.split_at(n);
        r = more;
        if let Ok(env) = Envelope::decode(chunk) {
            let _ = s.frame(1 + (i % 2), env, now);
        }
        now = now.saturating_add(997);
        let _ = s.tick(now);
        i = i.saturating_add(1);
    }
    let _ = s.closed(1, now);
    let _ = s.export();
    // a sealed session's open on the raw input
    let (mut j, a) = cc_live::crypto::JoinerHandshake::start(b"Q4M2X", "lan:K7");
    let mut h = cc_live::crypto::HostHandshake::new(b"Q4M2X", "lan:K7");
    if let Ok(b) = h.receive(&a)
        && let Ok(c) = j.receive(&b)
        && let Ok((_, mut hs)) = h.finish(&c)
    {
        let _ = hs.open(data);
    }
});
