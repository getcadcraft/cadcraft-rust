//! The local control channel — desktop-shell `control.rs` (CadCraft Studio 0.5) with exactly its rules, on our own
//! small HTTP/1.1 reader (no web server crate):
//!
//! listens on 127.0.0.1 only (port 47615, else the next free one) · `Authorization: Bearer <token>` on every request
//! (32 random bytes, base64url; compared in constant time; never in a URL) else 401 + WWW-Authenticate · Host other
//! than 127.0.0.1:<port> / localhost:<port> → 403 · any Origin header → 403 · POST /mcp (JSON answers, no SSE):
//! `initialize` issues Mcp-Session-Id (32 hex); later requests need it (missing 400, unknown 404); a notification or
//! response → 202; GET → 405; DELETE with the session → 204 · POST /control (same token, optional X-CadCraft-Session)
//! · JSON only (415), ≤ 4 MB (413), query strings 400 · anything else 404. An answer with error code -32002 (the person
//! pressed Disconnect) ends the session.
//!
//! On top of control.rs (AGENTS.md § 9): every refusal closes the connection (a request that is not HTTP, such as a bare
//! JSON line, is closed without an answer), at most 16 connections at a time, 30 s idle timeout, request heads capped
//! at 64 KB. The JSON-RPC message itself goes to `forward(message, session)`, which in the app runs it on the UI thread.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::token::{session_id, token_matches};

pub const DEFAULT_PORT: u16 = 47615;
pub const MAX_BODY: usize = 4 * 1024 * 1024;
pub const MAX_SESSIONS: usize = 32;
pub const MAX_CONNECTIONS: usize = 16;
const MAX_LINE: usize = 8 * 1024;
const MAX_HEAD: usize = 64 * 1024;
const MAX_HEADERS: usize = 100;
const IDLE: Duration = Duration::from_secs(30);

/// One HTTP request as read from a connection.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Request {
    pub method: String,
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// the body was over MAX_BODY (only the first MAX_BODY + 1 bytes were read)
    pub too_large: bool,
    /// the client asked to close (Connection: close, or HTTP/1.0 without keep-alive)
    pub close: bool,
}

impl Request {
    /// The first header of that name (case-insensitive), as tiny_http's lookup does.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// Why no request could be read.
#[derive(Debug, Clone, PartialEq)]
pub enum ReadError {
    /// the connection ended before a request started
    Closed,
    /// not HTTP, or over a limit: the connection is closed without an answer
    Bad(String),
}

fn bad(s: &str) -> ReadError {
    ReadError::Bad(s.into())
}

/// One line ending in LF (CRLF or LF), at most `MAX_LINE` bytes, without the line end. None at a clean end of input.
fn read_line<R: BufRead>(r: &mut R, head_left: &mut usize) -> Result<Option<String>, ReadError> {
    let mut buf = Vec::new();
    let mut limited = r.by_ref().take((MAX_LINE.min(*head_left) as u64).saturating_add(1));
    let n = limited.read_until(b'\n', &mut buf).map_err(|e| ReadError::Bad(format!("read: {e}")))?;
    if n == 0 {
        return Ok(None);
    }
    *head_left = head_left.saturating_sub(n);
    if buf.last() != Some(&b'\n') {
        return Err(bad("line too long or cut off"));
    }
    buf.pop();
    if buf.last() == Some(&b'\r') {
        buf.pop();
    }
    String::from_utf8(buf).map(Some).map_err(|_| bad("not UTF-8"))
}

fn is_token(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

/// Read one request (head + body) from a buffered stream.
pub fn read_request<R: BufRead>(r: &mut R) -> Result<Request, ReadError> {
    let mut head_left = MAX_HEAD;
    // tolerate empty lines before a request (RFC 9112 § 2.2)
    let line = loop {
        match read_line(r, &mut head_left)? {
            None => return Err(ReadError::Closed),
            Some(l) if l.is_empty() => continue,
            Some(l) => break l,
        }
    };
    let mut parts = line.split(' ');
    let (Some(method), Some(target), Some(version), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return Err(bad("not an HTTP request line"));
    };
    if !is_token(method) || !target.starts_with('/') || target.bytes().any(|b| b <= b' ' || b == 0x7f) {
        return Err(bad("not an HTTP request line"));
    }
    let http10 = match version {
        "HTTP/1.1" => false,
        "HTTP/1.0" => true,
        _ => return Err(bad("unsupported HTTP version")),
    };
    let mut headers: Vec<(String, String)> = Vec::new();
    loop {
        let Some(l) = read_line(r, &mut head_left)? else { return Err(bad("headers cut off")) };
        if l.is_empty() {
            break;
        }
        if headers.len() >= MAX_HEADERS {
            return Err(bad("too many headers"));
        }
        let Some((k, v)) = l.split_once(':') else { return Err(bad("bad header")) };
        if !is_token(k) {
            return Err(bad("bad header name"));
        }
        headers.push((k.to_string(), v.trim_matches(|c| c == ' ' || c == '\t').to_string()));
    }
    let mut req = Request { method: method.to_string(), target: target.to_string(), headers, ..Default::default() };
    let conn = req.header("Connection").unwrap_or_default().to_ascii_lowercase();
    req.close = conn.split(',').any(|t| t.trim() == "close") || (http10 && !conn.split(',').any(|t| t.trim() == "keep-alive"));
    let te = req.header("Transfer-Encoding").map(str::to_ascii_lowercase);
    let lengths: Vec<&str> =
        req.headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case("Content-Length")).map(|(_, v)| v.as_str()).collect();
    if te.is_some() && !lengths.is_empty() {
        return Err(bad("both Content-Length and Transfer-Encoding")); // request smuggling
    }
    let cap = (MAX_BODY as u64).saturating_add(1);
    if let Some(te) = te {
        if te != "chunked" {
            return Err(bad("unsupported Transfer-Encoding"));
        }
        let mut body = Vec::new();
        loop {
            let mut left = MAX_LINE;
            let Some(sz) = read_line(r, &mut left)? else { return Err(bad("chunk cut off")) };
            let hex = sz.split(';').next().unwrap_or_default().trim();
            if hex.is_empty() || hex.len() > 8 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(bad("bad chunk size"));
            }
            let n = u64::from_str_radix(hex, 16).map_err(|_| bad("bad chunk size"))?;
            if n == 0 {
                // trailers, then the end
                loop {
                    let mut left = MAX_LINE;
                    match read_line(r, &mut left)? {
                        Some(l) if l.is_empty() => break,
                        Some(_) => continue,
                        None => return Err(bad("chunk cut off")),
                    }
                }
                break;
            }
            let want = n.min(cap.saturating_sub(body.len() as u64));
            let got = r.by_ref().take(want).read_to_end(&mut body).map_err(|e| ReadError::Bad(format!("read: {e}")))?;
            if (got as u64) < want {
                return Err(bad("chunk cut off"));
            }
            if body.len() > MAX_BODY {
                req.too_large = true;
                req.body = body;
                return Ok(req);
            }
            let mut left = 4;
            if read_line(r, &mut left)? != Some(String::new()) {
                return Err(bad("bad chunk end"));
            }
        }
        req.body = body;
        return Ok(req);
    }
    if let Some(first) = lengths.first() {
        if lengths.iter().any(|l| l != first) || first.is_empty() || first.len() > 19 || !first.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad("bad Content-Length"));
        }
        let n: u64 = first.parse().map_err(|_| bad("bad Content-Length"))?;
        let want = n.min(cap);
        let mut body = Vec::new();
        let got = r.by_ref().take(want).read_to_end(&mut body).map_err(|e| ReadError::Bad(format!("read: {e}")))?;
        if (got as u64) < want {
            return Err(bad("body cut off"));
        }
        req.too_large = body.len() > MAX_BODY;
        req.body = body;
    }
    Ok(req)
}

/// Parse one request from bytes (the fuzz target's entry).
pub fn parse_request(bytes: &[u8]) -> Result<Request, ReadError> {
    read_request(&mut std::io::Cursor::new(bytes))
}

/// An answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    pub status: u16,
    pub body: Option<Value>,
    pub headers: Vec<(String, String)>,
}

impl Response {
    fn new(status: u16, body: Option<Value>, extra: &[(&str, String)]) -> Self {
        Response { status, body, headers: extra.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect() }
    }
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
    /// The bytes on the wire.
    pub fn to_bytes(&self, close: bool) -> Vec<u8> {
        let text = self.body.as_ref().map(crate::json::stringify).unwrap_or_default();
        let reason = match self.status {
            200 => "OK",
            202 => "Accepted",
            204 => "No Content",
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            413 => "Payload Too Large",
            415 => "Unsupported Media Type",
            _ => "Internal Server Error",
        };
        let mut h = format!("HTTP/1.1 {} {reason}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n", self.status);
        if self.body.is_some() {
            h.push_str("Content-Type: application/json\r\n");
        }
        if self.status != 204 {
            h.push_str(&format!("Content-Length: {}\r\n", text.len()));
        }
        for (k, v) in &self.headers {
            if k.eq_ignore_ascii_case("Connection") {
                continue;
            }
            // header values we write are ours; never let a CR/LF through anyway
            if v.bytes().any(|b| b == b'\r' || b == b'\n') {
                continue;
            }
            h.push_str(&format!("{k}: {v}\r\n"));
        }
        if close {
            h.push_str("Connection: close\r\n");
        }
        h.push_str("\r\n");
        let mut out = h.into_bytes();
        out.extend_from_slice(text.as_bytes());
        out
    }
}

/// The running server's state (control.rs `Running`).
#[derive(Debug)]
pub struct Running {
    pub port: u16,
    pub token: Mutex<String>,
    /// session id → last use
    pub sessions: Mutex<HashMap<String, Instant>>,
}

impl Running {
    pub fn new(port: u16, token: String) -> Self {
        Running { port, token: Mutex::new(token), sessions: Mutex::new(HashMap::new()) }
    }
    /// The person pressed Disconnect: that session ends (its next request gets 404).
    pub fn drop_session(&self, id: &str) -> bool {
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner).remove(id).is_some()
    }
    /// A new token: every session ends.
    pub fn set_token(&self, t: String) {
        *self.token.lock().unwrap_or_else(PoisonError::into_inner) = t;
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner).clear();
    }
}

fn rpc_err(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// The forward function: the JSON-RPC message and its session id → the answer (None: no answer), Err = it failed.
pub type Forward = dyn Fn(&Value, &str) -> Result<Option<Value>, ()> + Send + Sync;

/// One request, by the rules above (control.rs `handle`).
pub fn handle(forward: &Forward, run: &Running, req: &Request) -> Response {
    let null = Value::Null;
    let port = run.port;
    if req.too_large {
        return Response::new(413, Some(rpc_err(&null, -32000, "Too large")), &[]);
    }
    let host = req.header("Host").unwrap_or_default().to_ascii_lowercase();
    if host != format!("127.0.0.1:{port}") && host != format!("localhost:{port}") {
        return Response::new(403, Some(rpc_err(&null, -32000, "Forbidden host")), &[]);
    }
    if req.header("Origin").is_some() {
        return Response::new(403, Some(rpc_err(&null, -32000, "Requests from web pages are refused")), &[]);
    }
    let (path, query) = match req.target.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (req.target.as_str(), None),
    };
    if path != "/mcp" && path != "/control" {
        return Response::new(404, Some(rpc_err(&null, -32000, "Not found")), &[]);
    }
    let auth = req.header("Authorization").unwrap_or_default();
    let ok = auth.strip_prefix("Bearer ").is_some_and(|t| token_matches(t, &run.token.lock().unwrap_or_else(PoisonError::into_inner)));
    if !ok {
        let msg = rpc_err(&null, -32000, "A valid token is needed (Authorization: Bearer …)");
        return Response::new(401, Some(msg), &[("WWW-Authenticate", "Bearer realm=\"CadCraft Studio\"".into())]);
    }
    if query.is_some() {
        return Response::new(400, Some(rpc_err(&null, -32000, "No query strings")), &[]);
    }
    let mcp = path == "/mcp";
    let sid = req.header("Mcp-Session-Id").map(str::to_string);
    let method = req.method.to_ascii_uppercase();
    if method == "GET" {
        return Response::new(
            405,
            Some(rpc_err(&null, -32000, "No event stream here: POST JSON-RPC messages")),
            &[("Allow", "POST, DELETE".into())],
        );
    }
    let sessions = || run.sessions.lock().unwrap_or_else(PoisonError::into_inner);
    if method == "DELETE" && mcp {
        let gone = sid.as_ref().is_some_and(|s| sessions().remove(s).is_some());
        return if gone { Response::new(204, None, &[]) } else { Response::new(404, Some(rpc_err(&null, -32000, "No such session")), &[]) };
    }
    if method != "POST" {
        let allow = if mcp { "POST, DELETE" } else { "POST" };
        return Response::new(405, Some(rpc_err(&null, -32000, "POST only")), &[("Allow", allow.into())]);
    }
    let ctype = req.header("Content-Type").unwrap_or_default().to_ascii_lowercase();
    if !(ctype == "application/json" || ctype.starts_with("application/json;") || ctype.starts_with("application/json ")) {
        return Response::new(415, Some(rpc_err(&null, -32000, "Content-Type must be application/json")), &[]);
    }
    let Ok(msg) = serde_json::from_slice::<Value>(&req.body) else {
        return Response::new(400, Some(rpc_err(&null, -32700, "Parse error")), &[]);
    };
    if msg.is_array() {
        return Response::new(400, Some(rpc_err(&null, -32600, "Batches are not supported")), &[]);
    }
    if !msg.is_object() {
        return Response::new(400, Some(rpc_err(&null, -32600, "Invalid request")), &[]);
    }
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    let is_init = msg.get("method").and_then(Value::as_str) == Some("initialize");
    let session = if mcp {
        if is_init {
            let Ok(s) = session_id() else {
                return Response::new(500, Some(rpc_err(&id, -32603, "Internal error")), &[]);
            };
            let mut ss = sessions();
            if ss.len() >= MAX_SESSIONS
                && let Some(oldest) = ss.iter().min_by_key(|(_, t)| **t).map(|(k, _)| k.clone())
            {
                ss.remove(&oldest);
            }
            ss.insert(s.clone(), Instant::now());
            s
        } else {
            let Some(s) = sid else {
                return Response::new(400, Some(rpc_err(&id, -32000, "Mcp-Session-Id is required (initialize first)")), &[]);
            };
            if !sessions().contains_key(&s) {
                return Response::new(404, Some(rpc_err(&id, -32000, "This session has ended; initialize again")), &[]);
            }
            s
        }
    } else {
        let cid: String = req
            .header("X-CadCraft-Session")
            .unwrap_or("control")
            .chars()
            .take(64)
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        let s = format!("control:{}", if cid.is_empty() { "control" } else { &cid });
        sessions().entry(s.clone()).or_insert_with(Instant::now);
        s
    };
    if let Some(t) = sessions().get_mut(&session) {
        *t = Instant::now();
    }
    let answer = forward(&msg, &session).unwrap_or_else(|_| Some(rpc_err(&id, -32603, "Internal error")));
    let err_code = answer.as_ref().and_then(|a| a.get("error")).and_then(|e| e.get("code")).and_then(Value::as_i64);
    if (mcp && is_init && err_code.is_some()) || err_code == Some(-32002) {
        sessions().remove(&session);
    }
    let hdr: Vec<(&str, String)> = if mcp { vec![("Mcp-Session-Id", session.clone())] } else { vec![] };
    if msg.get("id").is_none() || msg.get("method").is_none() {
        return Response::new(202, None, &hdr);
    }
    let out = answer.unwrap_or_else(|| rpc_err(&id, -32603, "No answer"));
    Response::new(200, Some(out), &hdr)
}

/// Serve one connection until it closes, idles out or misbehaves.
fn connection(forward: &Forward, run: &Running, stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(IDLE));
    let _ = stream.set_write_timeout(Some(IDLE));
    let _ = stream.set_nodelay(true);
    let Ok(mut w) = stream.try_clone() else { return };
    let mut r = BufReader::new(stream);
    // not HTTP (a bare JSON line, a probe), over a limit, idle, or closed: no answer, the connection ends
    while let Ok(req) = read_request(&mut r) {
        let res = handle(forward, run, &req);
        let close = req.close || req.too_large || res.status >= 400;
        if w.write_all(&res.to_bytes(close)).is_err() || w.flush().is_err() || close {
            break;
        }
    }
    let _ = w.shutdown(Shutdown::Both);
}

/// A running server.
pub struct Server {
    pub run: Arc<Running>,
    stop: Arc<AtomicBool>,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server").field("port", &self.run.port).finish()
    }
}

impl Server {
    pub fn port(&self) -> u16 {
        self.run.port
    }
    /// The address assistants are given (the token goes in a header, never here).
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/mcp", self.run.port)
    }
    /// Stop accepting (connections already open finish their request).
    pub fn stop(&self) {
        if !self.stop.swap(true, Ordering::SeqCst) {
            // wake the accept loop
            let _ = TcpStream::connect_timeout(&std::net::SocketAddr::from(([127, 0, 0, 1], self.run.port)), Duration::from_millis(200));
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Start the server on 127.0.0.1: `port` (0 = any free port), else the next 19 ports up. Never another address.
pub fn start(port: u16, token: String, forward: Arc<Forward>) -> Result<Server, String> {
    let (listener, port) = if port == 0 {
        let l = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| format!("could not listen on 127.0.0.1: {e}"))?;
        let p = l.local_addr().map_err(|e| e.to_string())?.port();
        (l, p)
    } else {
        (port..port.saturating_add(20))
            .find_map(|p| TcpListener::bind(("127.0.0.1", p)).ok().map(|l| (l, p)))
            .ok_or_else(|| format!("no free port near {port} on 127.0.0.1"))?
    };
    let run = Arc::new(Running::new(port, token));
    let stop = Arc::new(AtomicBool::new(false));
    let (r2, s2) = (run.clone(), stop.clone());
    let live = Arc::new(AtomicUsize::new(0));
    std::thread::Builder::new()
        .name("cc-control".into())
        .spawn(move || {
            for conn in listener.incoming() {
                if s2.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = conn else { continue };
                // loopback only, even if the OS handed us something else
                if !stream.peer_addr().is_ok_and(|a| a.ip().is_loopback()) {
                    let _ = stream.shutdown(Shutdown::Both);
                    continue;
                }
                if live.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                    live.fetch_sub(1, Ordering::SeqCst);
                    let _ = stream.shutdown(Shutdown::Both);
                    continue;
                }
                let (r3, f3, l3) = (r2.clone(), forward.clone(), live.clone());
                let spawned = std::thread::Builder::new().name("cc-control-conn".into()).spawn(move || {
                    connection(&*f3, &r3, stream);
                    l3.fetch_sub(1, Ordering::SeqCst);
                });
                if spawned.is_err() {
                    live.fetch_sub(1, Ordering::SeqCst);
                }
            }
        })
        .map_err(|e| format!("could not start the control channel: {e}"))?;
    Ok(Server { run, stop })
}

/// (status, headers, body) of an answer.
pub type Answer = (u16, Vec<(String, String)>, Vec<u8>);

/// A small HTTP client for 127.0.0.1 (tests and `cadcraft-cli mcp --connect`): one request on its own connection →
/// (status, headers, body). Answers over 8 MB are refused.
pub fn request(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Result<Answer, String> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).map_err(|e| format!("could not connect to 127.0.0.1:{port}: {e}"))?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(150)));
    let mut head = format!("{method} {path} HTTP/1.1\r\nConnection: close\r\nContent-Length: {}\r\n", body.len());
    if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("Host")) {
        head.push_str(&format!("Host: 127.0.0.1:{port}\r\n"));
    }
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes()).and_then(|()| s.write_all(body)).map_err(|e| format!("could not send: {e}"))?;
    let mut r = BufReader::new(s);
    let mut left = MAX_HEAD;
    let line = read_line(&mut r, &mut left).map_err(|e| format!("{e:?}"))?.ok_or("the connection closed without an answer")?;
    let status: u16 = line.split(' ').nth(1).and_then(|c| c.parse().ok()).ok_or("not an HTTP answer")?;
    let mut hs = Vec::new();
    loop {
        let l = read_line(&mut r, &mut left).map_err(|e| format!("{e:?}"))?.ok_or("answer cut off")?;
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            hs.push((k.to_string(), v.trim().to_string()));
        }
    }
    let len = hs.iter().find(|(k, _)| k.eq_ignore_ascii_case("Content-Length")).and_then(|(_, v)| v.parse::<u64>().ok()).unwrap_or(0);
    if len > 8 * 1024 * 1024 {
        return Err("answer too large".into());
    }
    let mut body = Vec::new();
    r.take(len).read_to_end(&mut body).map_err(|e| format!("could not read the answer: {e}"))?;
    Ok((status, hs, body))
}

#[cfg(test)]
mod tests;
