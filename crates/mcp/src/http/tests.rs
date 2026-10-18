//! The control channel's rules, request by request (as desktop-shell control.rs and the reference
//! tools/mcp/live-server.mjs answer them), then the same over real sockets.

use super::*;
use std::io::{Read, Write};

const TOKEN: &str = "tok-0123456789abcdefghijklmnopqrstuvwxyzAB";

/// A stand-in window: answers every request with its method and session, notifications with nothing; `disconnect`
/// answers -32002 (the person pressed Disconnect); `fail` fails.
fn hub(m: &Value, s: &str) -> Result<Option<Value>, ()> {
    if m.get("id").is_none() {
        return Ok(None);
    }
    match m["method"].as_str() {
        Some("disconnect") => Ok(Some(json!({"jsonrpc": "2.0", "id": m["id"], "error": {"code": -32002, "message": "disconnected"}}))),
        Some("fail") => Err(()),
        Some("initialize") if m["params"]["refuse"] == true => {
            Ok(Some(json!({"jsonrpc": "2.0", "id": m["id"], "error": {"code": -32603, "message": "no"}})))
        }
        _ => Ok(Some(json!({"jsonrpc": "2.0", "id": m["id"], "result": {"method": m["method"], "session": s}}))),
    }
}

fn run() -> Running {
    Running::new(47615, TOKEN.into())
}

fn req(method: &str, target: &str, headers: &[(&str, &str)], body: &str) -> Request {
    let mut h: Vec<(String, String)> = vec![("Host".into(), "127.0.0.1:47615".into())];
    for (k, v) in headers {
        h.retain(|(x, _)| !x.eq_ignore_ascii_case(k));
        h.push(((*k).into(), (*v).into()));
    }
    Request { method: method.into(), target: target.into(), headers: h, body: body.as_bytes().to_vec(), too_large: false, close: false }
}

fn post(target: &str, extra: &[(&str, &str)], body: &str) -> Request {
    let auth = format!("Bearer {TOKEN}");
    let mut h: Vec<(&str, &str)> = vec![("Authorization", auth.as_str()), ("Content-Type", "application/json")];
    h.extend_from_slice(extra);
    req("POST", target, &h, body)
}

fn init(r: &Running) -> String {
    let res = handle(&hub, r, &post("/mcp", &[], r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#));
    assert_eq!(res.status, 200);
    res.header("Mcp-Session-Id").unwrap().to_string()
}

#[test]
fn tokens_are_required_and_checked() {
    let r = run();
    // missing
    let res = handle(&hub, &r, &req("POST", "/mcp", &[("Content-Type", "application/json")], "{}"));
    assert_eq!(res.status, 401);
    assert_eq!(res.header("WWW-Authenticate"), Some("Bearer realm=\"CadCraft Studio\""));
    assert_eq!(res.body.as_ref().unwrap()["error"]["message"], "A valid token is needed (Authorization: Bearer …)");
    // wrong (same length, one character off), shorter, other scheme
    let mut wrong = TOKEN.to_string();
    wrong.replace_range(0..1, "X");
    for auth in [
        format!("Bearer {wrong}"),
        format!("Bearer {}", &TOKEN[1..]),
        format!("Basic {TOKEN}"),
        format!("bearer {TOKEN}"),
        "Bearer ".into(),
    ] {
        let res = handle(&hub, &r, &req("POST", "/mcp", &[("Authorization", &auth), ("Content-Type", "application/json")], "{}"));
        assert_eq!(res.status, 401, "{auth}");
    }
    // in a URL it is never looked at: the query string itself is refused after the header check
    let res = handle(&hub, &r, &post(&format!("/mcp?token={TOKEN}"), &[], "{}"));
    assert_eq!(res.status, 400);
    assert_eq!(res.body.unwrap()["error"]["message"], "No query strings");
    let res = handle(&hub, &r, &req("POST", &format!("/mcp?token={TOKEN}"), &[("Content-Type", "application/json")], "{}"));
    assert_eq!(res.status, 401, "a token in the URL does not authorise");
    // a new token: the old one stops working and sessions end
    let sid = init(&r);
    r.set_token("other".into());
    assert_eq!(handle(&hub, &r, &post("/mcp", &[("Mcp-Session-Id", &sid)], r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#)).status, 401);
}

#[test]
fn host_and_origin() {
    let r = run();
    for host in ["evil.example:47615", "127.0.0.1:1234", "127.0.0.1", "", "[::1]:47615", "0.0.0.0:47615"] {
        let res = handle(&hub, &r, &post("/mcp", &[("Host", host)], "{}"));
        assert_eq!(res.status, 403, "{host}");
        assert_eq!(res.body.unwrap()["error"]["message"], "Forbidden host");
    }
    assert_eq!(
        handle(&hub, &r, &post("/mcp", &[("Host", "LOCALHOST:47615")], r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#)).status,
        200
    );
    for origin in ["https://getcadcraft.com", "null", ""] {
        let res = handle(&hub, &r, &post("/mcp", &[("Origin", origin)], "{}"));
        assert_eq!(res.status, 403, "{origin:?}");
        assert_eq!(res.body.unwrap()["error"]["message"], "Requests from web pages are refused");
    }
}

#[test]
fn paths_methods_and_bodies() {
    let r = run();
    assert_eq!(handle(&hub, &r, &post("/", &[], "{}")).status, 404);
    assert_eq!(handle(&hub, &r, &post("/mcp/", &[], "{}")).status, 404);
    let auth = format!("Bearer {TOKEN}");
    let get = handle(&hub, &r, &req("GET", "/mcp", &[("Authorization", &auth)], ""));
    assert_eq!((get.status, get.header("Allow")), (405, Some("POST, DELETE")));
    let put = handle(&hub, &r, &req("PUT", "/control", &[("Authorization", &auth)], ""));
    assert_eq!((put.status, put.header("Allow")), (405, Some("POST")));
    let plain = handle(&hub, &r, &req("POST", "/mcp", &[("Authorization", &auth), ("Content-Type", "text/plain")], "{}"));
    assert_eq!(plain.status, 415);
    assert_eq!(
        handle(
            &hub,
            &r,
            &req(
                "POST",
                "/mcp",
                &[("Authorization", &auth), ("Content-Type", "application/json; charset=utf-8")],
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#
            )
        )
        .status,
        200
    );
    let parse = handle(&hub, &r, &post("/mcp", &[], "LINE 0,0"));
    assert_eq!((parse.status, parse.body.unwrap()["error"]["code"].as_i64()), (400, Some(-32700)));
    let batch = handle(&hub, &r, &post("/mcp", &[], "[{}]"));
    assert_eq!(batch.body.unwrap()["error"]["message"], "Batches are not supported");
    let num = handle(&hub, &r, &post("/control", &[], "5"));
    assert_eq!((num.status, num.body.unwrap()["error"]["message"].as_str()), (400, Some("Invalid request")));
    let mut big = post("/mcp", &[], "{}");
    big.too_large = true;
    let res = handle(&hub, &r, &big);
    assert_eq!((res.status, res.body.unwrap()["error"]["message"].as_str()), (413, Some("Too large")));
}

#[test]
fn mcp_sessions() {
    let r = run();
    let no = handle(&hub, &r, &post("/mcp", &[], r#"{"jsonrpc":"2.0","id":5,"method":"tools/list"}"#));
    assert_eq!(no.status, 400);
    assert_eq!(
        no.body.unwrap(),
        json!({"jsonrpc": "2.0", "id": 5, "error": {"code": -32000, "message": "Mcp-Session-Id is required (initialize first)"}})
    );
    let unknown = handle(&hub, &r, &post("/mcp", &[("Mcp-Session-Id", "0123")], r#"{"jsonrpc":"2.0","id":5,"method":"tools/list"}"#));
    assert_eq!(unknown.status, 404);
    let sid = init(&r);
    assert_eq!(sid.len(), 32);
    assert!(sid.bytes().all(|b| b.is_ascii_hexdigit()));
    let ok = handle(&hub, &r, &post("/mcp", &[("Mcp-Session-Id", &sid)], r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#));
    assert_eq!(ok.status, 200);
    assert_eq!(ok.header("Mcp-Session-Id"), Some(sid.as_str()));
    assert_eq!(ok.body.unwrap()["result"]["session"], sid.as_str());
    // a notification (and a response) → 202, no body
    let note = handle(&hub, &r, &post("/mcp", &[("Mcp-Session-Id", &sid)], r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#));
    assert_eq!((note.status, note.body.is_none()), (202, true));
    // the window failing → -32603
    let failed = handle(&hub, &r, &post("/mcp", &[("Mcp-Session-Id", &sid)], r#"{"jsonrpc":"2.0","id":3,"method":"fail"}"#));
    assert_eq!(failed.body.unwrap()["error"]["code"], -32603);
    // Disconnect (-32002) ends the session
    let d = handle(&hub, &r, &post("/mcp", &[("Mcp-Session-Id", &sid)], r#"{"jsonrpc":"2.0","id":4,"method":"disconnect"}"#));
    assert_eq!(d.status, 200);
    assert_eq!(handle(&hub, &r, &post("/mcp", &[("Mcp-Session-Id", &sid)], r#"{"jsonrpc":"2.0","id":5,"method":"ping"}"#)).status, 404);
    // DELETE ends a session
    let sid2 = init(&r);
    let auth = format!("Bearer {TOKEN}");
    assert_eq!(handle(&hub, &r, &req("DELETE", "/mcp", &[("Authorization", &auth), ("Mcp-Session-Id", &sid2)], "")).status, 204);
    assert_eq!(handle(&hub, &r, &req("DELETE", "/mcp", &[("Authorization", &auth), ("Mcp-Session-Id", &sid2)], "")).status, 404);
    // an initialize the window refuses leaves no session behind
    let before = r.sessions.lock().unwrap().len();
    let bad = handle(&hub, &r, &post("/mcp", &[], r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"refuse":true}}"#));
    assert_eq!(bad.status, 200);
    assert_eq!(r.sessions.lock().unwrap().len(), before);
    // at most 32 sessions: the oldest goes
    for _ in 0..40 {
        init(&r);
    }
    assert_eq!(r.sessions.lock().unwrap().len(), MAX_SESSIONS);
    // the person's Disconnect from the app
    let sid3 = init(&r);
    assert!(r.drop_session(&sid3));
    assert_eq!(handle(&hub, &r, &post("/mcp", &[("Mcp-Session-Id", &sid3)], r#"{"jsonrpc":"2.0","id":5,"method":"ping"}"#)).status, 404);
}

#[test]
fn control_sessions() {
    let r = run();
    let res = handle(&hub, &r, &post("/control", &[], r#"{"jsonrpc":"2.0","id":1,"method":"control.hello"}"#));
    assert_eq!(res.body.unwrap()["result"]["session"], "control:control");
    assert!(res.headers.is_empty(), "no Mcp-Session-Id on /control");
    let res = handle(
        &hub,
        &r,
        &post("/control", &[("X-CadCraft-Session", "my-script_1/../\r\nx")], r#"{"jsonrpc":"2.0","id":1,"method":"app.info"}"#),
    );
    assert_eq!(res.body.unwrap()["result"]["session"], "control:my-script_1x");
}

#[test]
fn request_parser() {
    let ok = parse_request(b"POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:47615\r\nContent-Length: 2\r\n\r\n{}").unwrap();
    assert_eq!((ok.method.as_str(), ok.target.as_str(), ok.body.as_slice(), ok.close), ("POST", "/mcp", &b"{}"[..], false));
    let chunked = parse_request(b"POST /mcp HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}\r\n0\r\n\r\n").unwrap();
    assert_eq!(chunked.body, b"{}");
    assert!(parse_request(b"POST /mcp HTTP/1.0\r\n\r\n").unwrap().close);
    // not HTTP: a JSON line, an absolute URL, garbage, smuggling, cut off bodies
    for bad_input in [
        &b"{\"jsonrpc\":\"2.0\",\"method\":\"tools/list\"}\n"[..],
        b"GET http://evil/ HTTP/1.1\r\n\r\n",
        b"POST /mcp HTTP/2\r\n\r\n",
        b"POST /mcp HTTP/1.1\r\nContent-Length: 2\r\nTransfer-Encoding: chunked\r\n\r\n",
        b"POST /mcp HTTP/1.1\r\nContent-Length: 2\r\nContent-Length: 3\r\n\r\n{}",
        b"POST /mcp HTTP/1.1\r\nContent-Length: -1\r\n\r\n",
        b"POST /mcp HTTP/1.1\r\nContent-Length: 10\r\n\r\n{}",
        b"POST /mcp HTTP/1.1\r\nBad Header\r\n\r\n",
        b"POST /mcp HTTP/1.1\r\nTransfer-Encoding: gzip\r\n\r\n",
        b"POST /mcp HTTP/1.1\r\nHost: x",
    ] {
        assert!(matches!(parse_request(bad_input), Err(ReadError::Bad(_))), "{:?}", String::from_utf8_lossy(bad_input));
    }
    assert_eq!(parse_request(b""), Err(ReadError::Closed));
    // a head over 64 KB, a line over 8 KB, too many headers
    let long = format!("POST /mcp HTTP/1.1\r\nX: {}\r\n\r\n", "a".repeat(9000));
    assert!(parse_request(long.as_bytes()).is_err());
    let many = format!("POST /mcp HTTP/1.1\r\n{}\r\n", "X: y\r\n".repeat(150));
    assert!(parse_request(many.as_bytes()).is_err());
    // 4 MB + 1: marked too large, only MAX_BODY + 1 bytes read
    let mut big = format!("POST /mcp HTTP/1.1\r\nContent-Length: {}\r\n\r\n", MAX_BODY + 10).into_bytes();
    big.extend(std::iter::repeat_n(b' ', MAX_BODY + 10));
    let q = parse_request(&big).unwrap();
    assert!(q.too_large);
    assert_eq!(q.body.len(), MAX_BODY + 1);
    let mut exact = format!("POST /mcp HTTP/1.1\r\nContent-Length: {MAX_BODY}\r\n\r\n").into_bytes();
    exact.extend(std::iter::repeat_n(b' ', MAX_BODY));
    assert!(!parse_request(&exact).unwrap().too_large);
}

fn echo_forward() -> Arc<Forward> {
    Arc::new(|m: &Value, s: &str| hub(m, s))
}

fn raw(port: u16, bytes: &[u8]) -> Vec<u8> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let _ = s.write_all(bytes);
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    out
}

#[test]
fn over_sockets() {
    let srv = start(0, TOKEN.into(), echo_forward()).unwrap();
    let port = srv.port();
    let auth = format!("Bearer {TOKEN}");
    let host = format!("127.0.0.1:{port}");
    let (st, hs, body) = request(
        port,
        "POST",
        "/mcp",
        &[("Authorization", &auth), ("Content-Type", "application/json")],
        br#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
    )
    .unwrap();
    assert_eq!(st, 200);
    let sid = hs.iter().find(|(k, _)| k == "Mcp-Session-Id").map(|(_, v)| v.clone()).unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["result"]["session"], sid.as_str());
    // a bad token and an Origin
    assert_eq!(
        request(port, "POST", "/mcp", &[("Authorization", "Bearer nope"), ("Content-Type", "application/json")], b"{}").unwrap().0,
        401
    );
    assert_eq!(
        request(port, "POST", "/mcp", &[("Authorization", &auth), ("Origin", "https://x"), ("Content-Type", "application/json")], b"{}")
            .unwrap()
            .0,
        403
    );
    assert_eq!(
        request(port, "POST", "/mcp", &[("Host", "evil:1"), ("Authorization", &auth), ("Content-Type", "application/json")], b"{}")
            .unwrap()
            .0,
        403
    );
    // a bare JSON line (not HTTP): closed without a byte of answer
    assert!(raw(port, b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n").is_empty());
    // a refusal closes the connection: the second request on it gets nothing
    let two = format!(
        "POST /mcp HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{{}}POST /mcp HTTP/1.1\r\nHost: {host}\r\nAuthorization: {auth}\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{{}}"
    );
    let out = String::from_utf8_lossy(&raw(port, two.as_bytes())).into_owned();
    assert_eq!(out.matches("HTTP/1.1 ").count(), 1, "{out}");
    assert!(out.starts_with("HTTP/1.1 401") && out.contains("Connection: close"));
    // keep-alive: two good requests on one connection both answered
    let ok2 = format!(
        "POST /control HTTP/1.1\r\nHost: {host}\r\nAuthorization: {auth}\r\nContent-Type: application/json\r\nContent-Length: 39\r\n\r\n{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"a\"}}  POST /control HTTP/1.1\r\nHost: {host}\r\nAuthorization: {auth}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 37\r\n\r\n{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"b\"}}"
    );
    let out = String::from_utf8_lossy(&raw(port, ok2.as_bytes())).into_owned();
    assert_eq!(out.matches("HTTP/1.1 200").count(), 2, "{out}");
    // 4 MB + 1 → 413 and closed, without reading the rest
    let head = format!(
        "POST /mcp HTTP/1.1\r\nHost: {host}\r\nAuthorization: {auth}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        MAX_BODY + 1
    );
    let mut big = head.into_bytes();
    big.extend(std::iter::repeat_n(b'1', MAX_BODY + 1));
    let out = String::from_utf8_lossy(&raw(port, &big)).into_owned();
    assert!(out.starts_with("HTTP/1.1 413"), "{}", out.lines().next().unwrap_or_default());
    // exactly 4 MB is read (and refused as JSON for being a bare number run: still answered, not 413)
    let head = format!(
        "POST /control HTTP/1.1\r\nHost: {host}\r\nAuthorization: {auth}\r\nContent-Type: application/json\r\nContent-Length: {MAX_BODY}\r\n\r\n"
    );
    let mut four = head.into_bytes();
    four.extend(std::iter::repeat_n(b' ', MAX_BODY - 2));
    four.extend_from_slice(b"[]");
    let out = String::from_utf8_lossy(&raw(port, &four)).into_owned();
    assert!(out.starts_with("HTTP/1.1 400") && out.contains("Batches are not supported"), "{}", out.lines().next().unwrap_or_default());
    // only 127.0.0.1: the machine's other address does not answer
    if let Ok(sock) = std::net::UdpSocket::bind("0.0.0.0:0")
        && sock.connect("192.0.2.1:9").is_ok()
        && let Ok(addr) = sock.local_addr()
        && !addr.ip().is_loopback()
        && !addr.ip().is_unspecified()
    {
        assert!(
            TcpStream::connect_timeout(&std::net::SocketAddr::new(addr.ip(), port), Duration::from_millis(500)).is_err(),
            "answered on {}",
            addr.ip()
        );
    }
    srv.stop();
}
