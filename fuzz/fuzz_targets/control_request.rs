//! Fuzz the control channel's HTTP reader and its rules (cc_mcp::http; the bytes come from any local process that can
//! reach 127.0.0.1): any input must be read as one request or refused, and a request must get an answer by the rules
//! (token, Host, Origin, sessions, JSON only), never panic or read past its limits.
#![no_main]

use std::sync::OnceLock;

use cc_mcp::http::{Running, handle, parse_request};
use libfuzzer_sys::fuzz_target;
use serde_json::{Value, json};

const TOKEN: &str = "fuzz-token-0123456789abcdefghijklmnopqrstu";

fn running() -> &'static Running {
    static R: OnceLock<Running> = OnceLock::new();
    R.get_or_init(|| Running::new(47615, TOKEN.into()))
}

fn forward(m: &Value, s: &str) -> Result<Option<Value>, ()> {
    if m.get("id").is_none() {
        return Ok(None);
    }
    if m.get("method").and_then(Value::as_str) == Some("disconnect") {
        return Ok(Some(json!({"jsonrpc": "2.0", "id": m["id"], "error": {"code": -32002, "message": "gone"}})));
    }
    Ok(Some(json!({"jsonrpc": "2.0", "id": m["id"], "result": {"session": s}})))
}

fuzz_target!(|data: &[u8]| {
    if let Ok(req) = parse_request(data) {
        let res = handle(&forward, running(), &req);
        let _ = res.to_bytes(true);
    }
    // the same bytes as a body behind a valid head
    let mut framed = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:47615\r\nAuthorization: Bearer {TOKEN}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        data.len()
    )
    .into_bytes();
    framed.extend_from_slice(data);
    if let Ok(req) = parse_request(&framed) {
        let _ = handle(&forward, running(), &req).to_bytes(false);
    }
});
