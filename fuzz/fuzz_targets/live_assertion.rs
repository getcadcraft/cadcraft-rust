//! Fuzz signed-in names (cc_live::assertion, M16.12): a joiner's `hello.assertion` is any string a LAN peer sends. Any
//! text must check or be refused (format / key / signature / expired / audience) without panicking; the raw Ed25519
//! check takes any signature, message and key bytes.
#![no_main]

use cc_live::assertion::{ASSERTION_KEYS, ed25519_verify, from_b64url, picture_matches, verify_assertion};
use libfuzzer_sys::fuzz_target;

const TEST_KEYS: &[(&str, &str)] = &[("test0001", "dVxMuSVsp83ErP3Gz-7ahJAX5bn5UU6ZGRvWfgsNQnY"), ("a1f5b9cc", "e2Bo1-R1Hnd9kDkp3I7LvT1kZWGMm1Ha2K_XRQVW6DY")];

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = verify_assertion(&text, "QWERTYUIOPASDFGHJKLZXCVBNMqwertyuiopasdfgh", 1_793_000_000.0, TEST_KEYS);
    let _ = verify_assertion(&format!("v1.{text}"), "", f64::NAN, ASSERTION_KEYS);
    let _ = from_b64url(&text);
    if data.len() >= 96 {
        let (sig, rest) = data.split_at(64);
        let (key, msg) = rest.split_at(32);
        let _ = ed25519_verify(sig, msg, key);
    }
    let _ = picture_matches(data, &text);
});
