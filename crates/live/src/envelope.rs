//! The Live envelope (docs/LIVE-PROTOCOL.md § 2): `{t, s, c, n, q, b}` — written in that order with zeros, empty strings
//! and nil left out; read in any order, absent fields zero, unknown fields ignored.

use crate::msgpack::{DecodeError, EncodeError, Value, pack, unpack};

/// The protocol version this build speaks.
pub const PROTOCOL: u64 = 1;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Envelope {
    /// message type
    pub t: String,
    /// session id (only in `welcome`)
    pub s: String,
    /// a client id
    pub c: u64,
    /// the sender's counter (an edit's number; ping/pong clocks)
    pub n: u64,
    /// a seq
    pub q: u64,
    /// the body
    pub b: Value,
}

impl Envelope {
    pub fn new(t: &str) -> Self {
        Envelope { t: t.into(), b: Value::Nil, ..Default::default() }
    }
    pub fn body(mut self, b: Value) -> Self {
        self.b = b;
        self
    }
    /// As a map value, fields in order, zeros left out.
    pub fn to_value(&self) -> Value {
        let mut m: Vec<(String, Value)> = vec![("t".into(), Value::Str(self.t.clone()))];
        if !self.s.is_empty() {
            m.push(("s".into(), Value::Str(self.s.clone())));
        }
        for (k, v) in [("c", self.c), ("n", self.n), ("q", self.q)] {
            if v != 0 {
                m.push((k.into(), Value::from(v)));
            }
        }
        if !self.b.is_nil() {
            m.push(("b".into(), self.b.clone()));
        }
        Value::Map(m)
    }
    pub fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        pack(&self.to_value())
    }
    /// One envelope from a value: a map with a string `t`.
    pub fn from_value(v: &Value) -> Result<Envelope, DecodeError> {
        let Value::Map(_) = v else { return Err(DecodeError("live: an envelope is a map")) };
        let t = v.get("t").and_then(Value::as_str).ok_or(DecodeError("live: an envelope needs its type"))?;
        let num = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0);
        Ok(Envelope {
            t: t.to_string(),
            s: v.get("s").and_then(Value::as_str).unwrap_or("").to_string(),
            c: num("c"),
            n: num("n"),
            q: num("q"),
            b: v.get("b").cloned().unwrap_or(Value::Nil),
        })
    }
    /// Decode exactly one envelope (trailing bytes, truncation and unknown type bytes are refused).
    pub fn decode(bytes: &[u8]) -> Result<Envelope, DecodeError> {
        Envelope::from_value(&unpack(bytes)?)
    }
}
