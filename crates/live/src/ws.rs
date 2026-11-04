//! A small WebSocket (RFC 6455) on std::net for Studio Live on the LAN: the server's upgrade on `GET /live`, a client's
//! connect to `ws://host:port/live`, binary messages both ways, ping / pong / close. No TLS (the relay's wss:// is the
//! web edition's; the desktop has no HTTP client by decision), no extensions, no text messages (Live ignores them).
//!
//! Everything read is bounded: the HTTP head (16 KB, 64 header lines), a message (the caller's cap, checked against the
//! declared length before anything is allocated), fragments (at most 1024 per message), and a peer that stops talking
//! mid-frame times out on the socket's read timeout. Desktop only (the browser has its own WebSocket).

use std::io::{Read, Write};
use std::net::TcpStream;

use sha1::{Digest, Sha1};

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
/// The HTTP head of an upgrade, at most.
pub const MAX_HEAD: usize = 16 * 1024;
const MAX_HEADERS: usize = 64;
const MAX_FRAGMENTS: usize = 1024;

/// What went wrong on a WebSocket.
#[derive(Debug)]
pub enum WsError {
    Io(std::io::Error),
    /// The peer broke the protocol (bad head, bad frame, unmasked client frame, too big …).
    Protocol(&'static str),
    /// The peer closed the connection (a close frame or EOF).
    Closed,
}

impl std::fmt::Display for WsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WsError::Io(e) => write!(f, "{e}"),
            WsError::Protocol(w) => write!(f, "websocket: {w}"),
            WsError::Closed => f.write_str("websocket: closed"),
        }
    }
}
impl std::error::Error for WsError {}
impl From<std::io::Error> for WsError {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::UnexpectedEof { WsError::Closed } else { WsError::Io(e) }
    }
}

/// base64 (standard, padded) for the accept key and the client's key.
pub fn b64(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for chunk in b.chunks(3) {
        let (b0, b1, b2) = (chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0));
        let v = (u32::from(b0) << 16) | (u32::from(b1) << 8) | u32::from(b2);
        let c = |sh: u32| char::from(A.get(usize::try_from((v >> sh) & 63).unwrap_or(0)).copied().unwrap_or(b'A'));
        s.push(c(18));
        s.push(c(12));
        s.push(if chunk.len() > 1 { c(6) } else { '=' });
        s.push(if chunk.len() > 2 { c(0) } else { '=' });
    }
    s
}

/// `Sec-WebSocket-Accept` for a key (RFC 6455 § 4.2.2).
pub fn accept_key(key: &str) -> String {
    let mut h = Sha1::new();
    h.update(key.trim().as_bytes());
    h.update(GUID.as_bytes());
    b64(&h.finalize())
}

/// An HTTP request or response head: the first line and the headers (names lowercased).
#[derive(Debug, Clone, Default)]
pub struct Head {
    pub line: String,
    pub headers: Vec<(String, String)>,
}

impl Head {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

/// Reads an HTTP head (up to the blank line) byte by byte, so nothing after it is consumed.
pub fn read_head(s: &mut impl Read) -> Result<Head, WsError> {
    let mut buf: Vec<u8> = Vec::with_capacity(512);
    let mut one = [0u8; 1];
    loop {
        let n = s.read(&mut one)?;
        if n == 0 {
            return Err(WsError::Closed);
        }
        buf.extend_from_slice(&one);
        if buf.ends_with(b"\r\n\r\n") {
            break;
        }
        if buf.len() > MAX_HEAD {
            return Err(WsError::Protocol("head too long"));
        }
    }
    let text = String::from_utf8(buf).map_err(|_| WsError::Protocol("head not utf-8"))?;
    let mut lines = text.split("\r\n");
    let line = lines.next().unwrap_or("").to_string();
    let mut headers = Vec::new();
    for l in lines.filter(|l| !l.is_empty()) {
        if headers.len() >= MAX_HEADERS {
            return Err(WsError::Protocol("too many headers"));
        }
        let Some((k, v)) = l.split_once(':') else { return Err(WsError::Protocol("bad header")) };
        headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
    }
    Ok(Head { line, headers })
}

/// The server's side of the upgrade on a stream whose head was read: `GET /live` with `Upgrade: websocket` gets 101;
/// anything else gets 404 (Err: the caller closes).
pub fn accept(s: &mut TcpStream, head: &Head) -> Result<(), WsError> {
    let mut parts = head.line.split(' ');
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let upgrade = head.header("upgrade").is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
    let key = head.header("sec-websocket-key").filter(|k| !k.is_empty() && k.len() <= 64);
    let (true, true, Some(key)) = (method == "GET" && path == "/live", upgrade, key) else {
        let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot found");
        return Err(WsError::Protocol("not a live socket"));
    };
    let resp = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
        accept_key(key)
    );
    s.write_all(resp.as_bytes())?;
    Ok(())
}

/// The client's side: `GET /live` on `host:port` with a random key; the 101 and its accept key are checked.
pub fn connect(s: &mut TcpStream, host: &str, key16: &[u8; 16]) -> Result<(), WsError> {
    let key = b64(key16);
    let req = format!(
        "GET /live HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    );
    s.write_all(req.as_bytes())?;
    let head = read_head(s)?;
    if !head.line.starts_with("HTTP/1.1 101") {
        return Err(WsError::Protocol("the server did not switch to a websocket"));
    }
    if head.header("sec-websocket-accept") != Some(accept_key(&key).as_str()) {
        return Err(WsError::Protocol("wrong accept key"));
    }
    Ok(())
}

/// One message read.
#[derive(Debug, PartialEq, Eq)]
pub enum Msg {
    Binary(Vec<u8>),
    /// A text message (not part of Live: ignored by the caller).
    Text,
    Ping(Vec<u8>),
    Pong,
}

/// Reads one frame header + payload; `masked`: what the peer must do (clients mask, servers don't).
fn read_frame(s: &mut impl Read, room: usize, masked: bool) -> Result<(bool, u8, Vec<u8>), WsError> {
    let mut h = [0u8; 2];
    s.read_exact(&mut h)?;
    let fin = h[0] & 0x80 != 0;
    if h[0] & 0x70 != 0 {
        return Err(WsError::Protocol("reserved bits set"));
    }
    let op = h[0] & 0x0f;
    let is_masked = h[1] & 0x80 != 0;
    if is_masked != masked {
        return Err(WsError::Protocol(if masked { "an unmasked client frame" } else { "a masked server frame" }));
    }
    let len = match h[1] & 0x7f {
        126 => {
            let mut b = [0u8; 2];
            s.read_exact(&mut b)?;
            u64::from(u16::from_be_bytes(b))
        }
        127 => {
            let mut b = [0u8; 8];
            s.read_exact(&mut b)?;
            u64::from_be_bytes(b)
        }
        n => u64::from(n),
    };
    let control = op & 0x08 != 0;
    if control && (len > 125 || !fin) {
        return Err(WsError::Protocol("bad control frame"));
    }
    let len = usize::try_from(len).map_err(|_| WsError::Protocol("frame too big"))?;
    if len > room {
        return Err(WsError::Protocol("message too big"));
    }
    let mut key = [0u8; 4];
    if masked {
        s.read_exact(&mut key)?;
    }
    let mut data = vec![0u8; len];
    s.read_exact(&mut data)?;
    if masked {
        for (b, k) in data.iter_mut().zip(key.iter().cycle()) {
            *b ^= k;
        }
    }
    Ok((fin, op, data))
}

/// Reads the next message (fragments joined), at most `max` bytes. `masked`: the peer is a client (server side).
pub fn read_msg(s: &mut impl Read, max: usize, masked: bool) -> Result<Msg, WsError> {
    let mut msg: Vec<u8> = Vec::new();
    let mut first: Option<u8> = None;
    let mut fragments = 0usize;
    loop {
        let room = max.saturating_sub(msg.len()).max(125);
        let (fin, op, data) = read_frame(s, room, masked)?;
        match op {
            0x8 => return Err(WsError::Closed),
            0x9 => return Ok(Msg::Ping(data)),
            0xA => return Ok(Msg::Pong),
            0x0 => {
                if first.is_none() {
                    return Err(WsError::Protocol("a continuation with nothing to continue"));
                }
            }
            0x1 | 0x2 => {
                if first.is_some() {
                    return Err(WsError::Protocol("a new message inside a fragmented one"));
                }
                first = Some(op);
            }
            _ => return Err(WsError::Protocol("unknown opcode")),
        }
        fragments = fragments.saturating_add(1);
        if fragments > MAX_FRAGMENTS || msg.len().saturating_add(data.len()) > max {
            return Err(WsError::Protocol("message too big"));
        }
        msg.extend_from_slice(&data);
        if fin {
            return Ok(if first == Some(0x1) { Msg::Text } else { Msg::Binary(msg) });
        }
    }
}

/// Writes one frame. `mask`: a client masks with `key`; a server doesn't.
fn write_frame(s: &mut impl Write, op: u8, data: &[u8], mask: Option<[u8; 4]>) -> Result<(), WsError> {
    let mut head: Vec<u8> = Vec::with_capacity(14);
    head.push(0x80 | op);
    let mbit = if mask.is_some() { 0x80 } else { 0 };
    let len = data.len();
    if len < 126 {
        head.push(mbit | u8::try_from(len).unwrap_or(0));
    } else if let Ok(l16) = u16::try_from(len) {
        head.push(mbit | 126);
        head.extend_from_slice(&l16.to_be_bytes());
    } else {
        head.push(mbit | 127);
        head.extend_from_slice(&u64::try_from(len).unwrap_or(u64::MAX).to_be_bytes());
    }
    match mask {
        Some(k) => {
            head.extend_from_slice(&k);
            let masked: Vec<u8> = data.iter().zip(k.iter().cycle()).map(|(b, m)| b ^ m).collect();
            head.extend_from_slice(&masked);
        }
        None => head.extend_from_slice(data),
    }
    s.write_all(&head)?;
    Ok(())
}

/// A binary message (one frame).
pub fn write_binary(s: &mut impl Write, data: &[u8], mask: Option<[u8; 4]>) -> Result<(), WsError> {
    write_frame(s, 0x2, data, mask)
}
pub fn write_pong(s: &mut impl Write, data: &[u8], mask: Option<[u8; 4]>) -> Result<(), WsError> {
    write_frame(s, 0xA, data.get(..data.len().min(125)).unwrap_or(&[]), mask)
}
/// A close frame (1000, or 1008 with a short reason).
pub fn write_close(s: &mut impl Write, code: u16, reason: &str, mask: Option<[u8; 4]>) -> Result<(), WsError> {
    let mut p = code.to_be_bytes().to_vec();
    p.extend(reason.bytes().take(120));
    write_frame(s, 0x8, &p, mask)
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn the_accept_key_is_rfc_6455s() {
        // RFC 6455 § 1.3's example
        assert_eq!(accept_key("dGhlIHNhbXBsZSBub25jZQ=="), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
        assert_eq!(b64(b"ab"), "YWI=");
        assert_eq!(b64(b"abc"), "YWJj");
    }

    #[test]
    fn frames_round_trip_masked_and_not() {
        for n in [0usize, 5, 125, 126, 300, 70_000] {
            let data: Vec<u8> = (0..n).map(|i| (i % 251) as u8).collect();
            let mut buf = Vec::new();
            write_binary(&mut buf, &data, Some([1, 2, 3, 4])).expect("write");
            assert_eq!(read_msg(&mut buf.as_slice(), 1 << 20, true).expect("read"), Msg::Binary(data.clone()));
            let mut buf = Vec::new();
            write_binary(&mut buf, &data, None).expect("write");
            assert_eq!(read_msg(&mut buf.as_slice(), 1 << 20, false).expect("read"), Msg::Binary(data));
        }
    }

    #[test]
    fn hostile_frames_are_refused() {
        // unmasked frame to a server
        let mut buf = Vec::new();
        write_binary(&mut buf, b"x", None).expect("write");
        assert!(matches!(read_msg(&mut buf.as_slice(), 100, true), Err(WsError::Protocol(_))));
        // a declared length beyond the cap is refused before any allocation
        let big = [0x82u8, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
        assert!(matches!(read_msg(&mut big.as_slice(), 1 << 20, false), Err(WsError::Protocol(_))));
        // truncated
        assert!(matches!(read_msg(&mut [0x82u8, 0x05, 1].as_slice(), 100, false), Err(WsError::Closed)));
        // a close frame
        let mut buf = Vec::new();
        write_close(&mut buf, 1000, "bye", None).expect("write");
        assert!(matches!(read_msg(&mut buf.as_slice(), 100, false), Err(WsError::Closed)));
        // a head without its blank line, and one too long
        assert!(read_head(&mut b"GET / HTTP/1.1\r\n".as_slice()).is_err());
        let long = vec![b'a'; MAX_HEAD + 10];
        assert!(matches!(read_head(&mut long.as_slice()), Err(WsError::Protocol(_))));
    }
}
