//! Fuzz Studio Live's LAN discovery (cc_live::mdns, M16.12): any machine on the network can send any packet to port
//! 5353. Any bytes must parse as a DNS message or be dropped, and an answer to whatever parses is bounded, without
//! panicking; what parses resolves (or doesn't) for any tag.
#![no_main]

use cc_live::mdns::{Service, encode, parse, resolve};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some(m) = parse(data) {
        let _ = encode(&m, true);
        let svc = Service { tag: "K7".into(), app: "cadcraft".into(), port: 4000, host: "ccstudio-00000000.local.".into(), addrs: vec![std::net::Ipv4Addr::new(10, 0, 0, 2)] };
        if let Some(a) = svc.answer(&m) {
            let _ = encode(&a, false);
        }
        let _ = resolve(&[m], "K7");
    }
});
