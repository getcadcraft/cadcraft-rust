//! Signed-in names: every case of tests/assertion_vectors.json (tokens signed with a test key by noble, and their
//! mutations, made by forks/tools/live-studiofin/assertion-vectors.mjs) gets 0.5 live-crypto.js verifyAssertion's answer.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use cc_live::assertion::{ASSERTION_KEYS, picture_matches, verify_assertion};

#[test]
fn verify_assertion_answers_as_live_crypto() {
    let d: serde_json::Value = serde_json::from_str(include_str!("assertion_vectors.json")).unwrap();
    let keys: Vec<(String, String)> =
        d["keys"].as_array().unwrap().iter().map(|k| (k[0].as_str().unwrap().to_string(), k[1].as_str().unwrap().to_string())).collect();
    let keys: Vec<(&str, &str)> = keys.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let mut n = 0;
    for c in d["cases"].as_array().unwrap() {
        let r = verify_assertion(c["token"].as_str().unwrap(), c["aud"].as_str().unwrap(), c["now"].as_f64().unwrap(), &keys);
        match r {
            Ok(s) => {
                assert!(c["ok"].as_bool().unwrap(), "{c}: accepted, 0.5 says {}", c["reason"]);
                assert_eq!(s.name, c["name"].as_str().unwrap());
                assert_eq!(s.verified, c["verified"].as_bool().unwrap());
                assert_eq!(s.pic.as_deref(), c["pic"].as_str());
            }
            Err(e) => {
                assert!(!c["ok"].as_bool().unwrap(), "{c}: refused {e:?}, 0.5 accepts");
                assert_eq!(e.as_str(), c["reason"].as_str().unwrap(), "{c}");
            }
        }
        n += 1;
    }
    assert!(n >= 80);
    // the production key never accepts a test key's token
    let first = d["cases"][0]["token"].as_str().unwrap();
    assert_eq!(
        verify_assertion(first, d["cases"][0]["aud"].as_str().unwrap(), d["cases"][0]["now"].as_f64().unwrap(), ASSERTION_KEYS)
            .map(|_| ())
            .unwrap_err()
            .as_str(),
        "key"
    );
    let pic: Vec<u8> = (0..d["pic"]["hex"].as_str().unwrap().len() / 2)
        .map(|i| u8::from_str_radix(&d["pic"]["hex"].as_str().unwrap()[2 * i..2 * i + 2], 16).unwrap())
        .collect();
    assert!(picture_matches(&pic, d["pic"]["sha"].as_str().unwrap()));
    assert!(!picture_matches(&pic[1..], d["pic"]["sha"].as_str().unwrap()));
}
