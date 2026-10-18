//! docs/LIVE-PROTOCOL.md § 13: the shared test vectors (`tools/live-test/vectors.json` of the CadCraft site, version 1),
//! the Rust fork's side. The copy in `tests/vectors.json` is checked against the site's file when that is present.

use cc_live::envelope::Envelope;
use cc_live::msgpack::{Value, pack, unpack};
use cc_live::session::{Core, Decision, Role};
use serde_json::Value as J;

const SITE: &str = "/var/www/CADCRAFT/getcadcraft.com/tools/live-test/vectors.json";

fn vectors() -> J {
    serde_json::from_str(include_str!("vectors.json")).unwrap()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}
fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// vectors.json values: bytes as {"$bin": hex}, big integers as {"$int": "…"}.
fn to_value(j: &J) -> Value {
    match j {
        J::Null => Value::Nil,
        J::Bool(b) => Value::Bool(*b),
        J::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i128::from(i))
            } else if let Some(u) = n.as_u64() {
                Value::Int(i128::from(u))
            } else {
                let f = n.as_f64().unwrap();
                if f.fract() == 0.0 && f.abs() <= 9_007_199_254_740_991.0 { Value::Int(f as i128) } else { Value::F64(f) }
            }
        }
        J::String(s) => Value::Str(s.clone()),
        J::Array(a) => Value::Arr(a.iter().map(to_value).collect()),
        J::Object(o) => {
            if o.len() == 1 {
                if let Some(J::String(h)) = o.get("$bin") {
                    return Value::Bin(unhex(h));
                }
                if let Some(J::String(i)) = o.get("$int") {
                    return Value::Int(i.parse().unwrap());
                }
            }
            Value::Map(o.iter().map(|(k, v)| (k.clone(), to_value(v))).collect())
        }
    }
}

/// Values compare as JS sees them: numbers by value whatever their width.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Int(x), Value::F64(y)) | (Value::F64(y), Value::Int(x)) => (*x as f64) == *y,
        (Value::Arr(x), Value::Arr(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q)),
        (Value::Map(x), Value::Map(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.iter().any(|(k2, v2)| k == k2 && same(v, v2))),
        _ => a == b,
    }
}

#[test]
fn the_copy_is_the_sites() {
    match std::fs::read_to_string(SITE) {
        Ok(site) => {
            assert_eq!(serde_json::from_str::<J>(&site).unwrap(), vectors(), "tests/vectors.json differs from {SITE}: copy it again")
        }
        Err(_) => eprintln!("skipped: {SITE} is not here"),
    }
}

#[test]
fn envelopes_encode_and_decode_like_js() {
    let v = vectors();
    assert_eq!(v["protocol"], 1);
    let list = v["envelopes"].as_array().unwrap();
    assert!(list.len() >= 50);
    for e in list {
        let name = e["name"].as_str().unwrap();
        let bytes = unhex(e["hex"].as_str().unwrap());
        let want = to_value(&e["value"]);
        let got = unpack(&bytes).unwrap_or_else(|err| panic!("{name}: {err}"));
        assert!(same(&got, &want), "{name}: decoded {got:?}");
        assert_eq!(hex(&pack(&want).unwrap()), hex(&bytes), "{name}: value encoding");
        let env = Envelope::decode(&bytes).unwrap_or_else(|err| panic!("{name}: {err}"));
        assert_eq!(hex(&env.encode().unwrap()), hex(&bytes), "{name}: envelope round trip");
    }
}

#[test]
fn other_valid_encodings_decode() {
    for e in vectors()["decode_only"].as_array().unwrap() {
        let name = e["name"].as_str().unwrap();
        let bytes = unhex(e["hex"].as_str().unwrap());
        let got = unpack(&bytes).unwrap_or_else(|err| panic!("{name}: {err}"));
        assert!(same(&got, &to_value(&e["value"])), "{name}: {got:?}");
        Envelope::decode(&bytes).unwrap_or_else(|err| panic!("{name}: {err}"));
    }
}

#[test]
fn bad_inputs_are_refused() {
    for e in vectors()["bad"].as_array().unwrap() {
        let name = e["name"].as_str().unwrap();
        assert!(unpack(&unhex(e["hex"].as_str().unwrap())).is_err(), "{name} was accepted");
    }
    // the properties live-client.js's tests pin beyond the vectors
    assert_eq!(hex(&pack(&Value::Map(vec![("z".into(), Value::Int(1)), ("a".into(), Value::Int(2))])).unwrap()), "82a17a01a16102");
    assert!(matches!(unpack(&unhex("81a95f5f70726f746f5f5f81a178c3")).unwrap(), Value::Map(m) if m.is_empty()));
    assert!(unpack(&[0x91u8; 1000]).is_err(), "deep nesting");
    assert!(unpack(&unhex("ddffffffff")).is_err(), "a huge count with no data");
}

#[test]
fn scenarios_decide_as_the_server_does() {
    let v = vectors();
    let list = v["scenarios"].as_array().unwrap();
    assert!(list.len() >= 10);
    for s in list {
        let name = s["name"].as_str().unwrap();
        let mut core = Core::new();
        let mut ids = std::collections::HashMap::new();
        for (i, c) in s["clients"].as_array().unwrap().iter().enumerate() {
            let c = c.as_str().unwrap().to_string();
            let id = core.add(&c, if i == 0 { Role::Host } else { Role::Editor });
            ids.insert(c, id);
        }
        let mut n = std::collections::HashMap::<u64, u64>::new();
        for (k, st) in s["steps"].as_array().unwrap().iter().enumerate() {
            let by = ids[st["by"].as_str().unwrap()];
            let keys = |f: &str| st[f].as_array().map(|a| a.iter().map(|x| x.as_str().unwrap().to_string()).collect::<Vec<_>>());
            if let Some(h) = keys("hold") {
                core.hold(by, &h, 0);
                continue;
            }
            if let Some(h) = keys("release") {
                core.release(by, Some(&h));
                continue;
            }
            if let Some(h) = keys("lock") {
                core.lock(by, &h, true).unwrap();
                continue;
            }
            if let Some(h) = keys("unlock") {
                core.lock(by, &h, false).unwrap();
                continue;
            }
            if let Some(r) = st.get("role") {
                let who = ids[r["who"].as_str().unwrap()];
                core.set_role(by, who, Role::parse(r["role"].as_str().unwrap()).unwrap()).unwrap();
                continue;
            }
            let next = n.entry(by).or_insert(0);
            *next += 1;
            let mut e = Envelope::new("edit").body(Value::map(vec![
                ("label", Value::Str(st["label"].as_str().unwrap_or("edit").into())),
                ("ops", Value::Arr(vec![])),
                ("touched", to_value(&st["touched"])),
            ]));
            e.n = *next;
            // through the wire format, as a transport carries it
            let e = Envelope::decode(&e.encode().unwrap()).unwrap();
            let (d, _) = core.edit(by, &e);
            let exp = &st["expect"];
            if let Some(q) = exp.get("commit") {
                assert_eq!(d, Decision::Commit { q: q.as_u64().unwrap() }, "{name}, step {k}");
            } else {
                let reason = exp["refuse"].as_str().unwrap();
                match d {
                    Decision::Refuse { reason: r, by: who } => {
                        assert_eq!(r, reason, "{name}, step {k}");
                        if let Some(b) = exp.get("by") {
                            assert_eq!(who, Some(ids[b.as_str().unwrap()]), "{name}, step {k}: by");
                        }
                    }
                    other => panic!("{name}, step {k}: {other:?}, expected refuse {reason}"),
                }
            }
        }
    }
}
