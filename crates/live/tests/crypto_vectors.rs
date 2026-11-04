//! LIVE-CRYPTO.md's shared vectors (crypto-vectors.json: a copy of the 0.5 site's tools/live-crypto/vectors.json,
//! which 0.5's desktop crypto.rs and live-crypto.js both check): with the same random bytes, the fork's handshake and
//! frames are byte for byte those of 0.5 — so a 0.5 joiner (or host) and a fork one interoperate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use cc_live::crypto::{self, HostHandshake, JoinerHandshake, Opened};
use serde_json::Value as J;

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

/// The vector's random bytes, in order (the spake2 crate draws 64 for its scalar).
struct Fixed(Vec<u8>, usize);
impl rand_core::RngCore for Fixed {
    fn next_u32(&mut self) -> u32 {
        let mut b = [0u8; 4];
        self.fill_bytes(&mut b);
        u32::from_le_bytes(b)
    }
    fn next_u64(&mut self) -> u64 {
        let mut b = [0u8; 8];
        self.fill_bytes(&mut b);
        u64::from_le_bytes(b)
    }
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        for d in dest.iter_mut() {
            *d = self.0[self.1];
            self.1 += 1;
        }
    }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}
impl rand_core::CryptoRng for Fixed {}

#[test]
fn the_handshake_and_frames_are_0_5s_byte_for_byte() {
    let v: J = serde_json::from_str(include_str!("crypto-vectors.json")).unwrap();
    assert_eq!(v["format"], "cadcraft-live-crypto-vectors");
    let mut cases = 0;
    for c in v["cases"].as_array().unwrap() {
        let ctx = c["context"].as_str().unwrap();
        let (mut j, a) = JoinerHandshake::start_with_rng(
            &unhex(c["joiner_secret"].as_str().unwrap()),
            ctx,
            Fixed(unhex(c["joiner_rng"].as_str().unwrap()), 0),
        );
        assert_eq!(a, unhex(c["a"].as_str().unwrap()), "{}: A", c["name"]);
        let mut h = HostHandshake::new(&unhex(c["host_secret"].as_str().unwrap()), ctx);
        let b = h.receive_with_rng(&a, Fixed(unhex(c["host_rng"].as_str().unwrap()), 0)).unwrap();
        assert_eq!(b, unhex(c["b"].as_str().unwrap()), "{}: B", c["name"]);
        let jc = j.receive(&b).unwrap();
        assert_eq!(jc, unhex(c["joiner_confirm"].as_str().unwrap()), "{}: joiner confirm", c["name"]);
        if c["host_result"] == "wrong_code" {
            assert_eq!(h.finish(&jc).err(), Some(crypto::CryptoError::WrongCode));
            cases += 1;
            continue;
        }
        let (hc, mut hs) = h.finish(&jc).unwrap();
        assert_eq!(hc, unhex(c["host_confirm"].as_str().unwrap()), "{}: host confirm", c["name"]);
        let mut js = j.finish(&hc).unwrap();
        assert_eq!(js.exporter("ticket").to_vec(), unhex(c["exporter_ticket"].as_str().unwrap()));
        for f in c["frames"].as_array().unwrap() {
            let body = unhex(f["body"].as_str().unwrap());
            let want = unhex(f["frame"].as_str().unwrap());
            let joiner = f["from"] == "joiner";
            let (tx, rx) = if joiner { (&mut js, &mut hs) } else { (&mut hs, &mut js) };
            let got = if f["kind"] == "rekey" { tx.rekey().unwrap() } else { tx.seal(&body).unwrap() };
            assert_eq!(got, want, "{}: frame", c["name"]);
            let opened = rx.open(&got).unwrap();
            assert_eq!(opened, if f["kind"] == "rekey" { Opened::Rekeyed } else { Opened::Data(body) });
        }
        cases += 1;
    }
    assert_eq!(cases, 4);
    for k in v["codes"].as_array().unwrap() {
        let got = crypto::parse_code(k["code"].as_str().unwrap());
        match k["secret"].as_str() {
            Some(s) => assert_eq!(got, Some((k["tag"].as_str().unwrap().to_string(), unhex(s)))),
            None => assert_eq!(got, None, "{}", k["code"]),
        }
    }
    for k in v["links"].as_array().unwrap() {
        assert_eq!(crypto::parse_link_secret(k["fragment"].as_str().unwrap()), k["secret"].as_str().map(unhex), "{}", k["fragment"]);
    }
}
