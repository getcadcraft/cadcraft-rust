//! Studio Live on the LAN, desktop side (LIVE-PROTOCOL.md § 1, § 5): the session server's transport and a joiner's
//! connection, on std threads (no async runtime).
//!
//! Server ([`start`]): one thread owns the [`Session`], the clock and every connection's frame keys (open on the way
//! in, seal on the way out); an accept thread; per connection a reader thread (the WebSocket upgrade on `GET /live`,
//! then one message at a time) and a writer thread. The host's own window and a replica attach in-process
//! ([`LiveHandle::attach_local`]) and exchange envelopes without frames. It listens only when the person starts a
//! session, on the address the caller gives.
//!
//! Joiner ([`Joiner::connect`]): the WebSocket to `ws://host:port/live`, the handshake with a short code (or a rejoin
//! ticket), then sealed envelopes both ways; `plain` for test servers.
//!
//! Limits: 64 connections at a time, a message at most 1 MB + 64 KB from the LAN (256 MB from loopback, a test
//! harness's replica sends whole snapshots), the WebSocket head 16 KB, 30 s without any frame closes a connection
//! (the session itself drops a client after 6 s without one, § 8).

use std::collections::HashMap;
use std::io::Write;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};

use crate::crypto;
use crate::envelope::Envelope;
use crate::msgpack::Value;
use crate::server::{Config, ConnId, Out, Session, Stats};
use crate::ws;

/// Largest message a LAN peer may send (an edit is at most 1 MB, § 10; plus envelope and frame overhead).
pub const MAX_FRAME: usize = 1024 * 1024 + 64 * 1024;
/// Loopback peers (a test harness's replica sends whole snapshots).
pub const MAX_FRAME_LOCAL: usize = 256 * 1024 * 1024;
/// Connections at a time (20 people, their assistants, handshakes under way).
pub const MAX_CONNS: usize = 64;
/// A connection with no frame at all for this long is closed (the session's own pings come every 2 s).
pub const IDLE: Duration = Duration::from_secs(30);
const TICK: Duration = Duration::from_millis(8);

enum Event {
    Open(ConnId, Sender<Wire>, bool, &'static str),
    Frame(ConnId, Vec<u8>),
    Local(ConnId, Vec<u8>),
    Closed(ConnId),
    Stats(Sender<(Stats, u64, usize)>),
    Export(Sender<Value>),
    Stop,
}

/// To a connection's writer (network) or to an in-process client.
#[derive(Debug)]
pub enum Wire {
    /// Network: frame bytes as sent. In-process: one encoded envelope.
    Bytes(Vec<u8>),
    Close(String),
}

/// A running session server.
#[derive(Clone)]
pub struct LiveHandle {
    pub addr: SocketAddr,
    pub session_id: String,
    tx: Sender<Event>,
    next_conn: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
}

/// An in-process client (the host's window, the replica): envelopes in and out, no frames.
pub struct LocalClient {
    pub conn: ConnId,
    tx: Sender<Event>,
    pub rx: Receiver<Wire>,
}

impl LocalClient {
    pub fn send(&self, env: &Envelope) {
        if let Ok(b) = env.encode() {
            let _ = self.tx.send(Event::Local(self.conn, b));
        }
    }
    pub fn close(&self) {
        let _ = self.tx.send(Event::Closed(self.conn));
    }
    /// The next envelope from the session within `wait` (None: nothing yet, or the session closed this client).
    pub fn recv(&self, wait: Duration) -> Option<Envelope> {
        match self.rx.recv_timeout(wait) {
            Ok(Wire::Bytes(b)) => Envelope::decode(&b).ok(),
            _ => None,
        }
    }
}

impl LiveHandle {
    pub fn attach_local(&self) -> LocalClient {
        let conn = self.next_conn.fetch_add(1, Ordering::Relaxed);
        let (wtx, wrx) = channel();
        let _ = self.tx.send(Event::Open(conn, wtx, true, "local"));
        LocalClient { conn, tx: self.tx.clone(), rx: wrx }
    }
    /// (stats, seq, connections) — for tests and the host's status line.
    pub fn stats(&self) -> Option<(Stats, u64, usize)> {
        let (tx, rx) = channel();
        self.tx.send(Event::Stats(tx)).ok()?;
        rx.recv_timeout(Duration::from_secs(5)).ok()
    }
    /// The session's state for a host handoff (`Session::export`).
    pub fn export_state(&self) -> Option<Value> {
        let (tx, rx) = channel();
        self.tx.send(Event::Export(tx)).ok()?;
        rx.recv_timeout(Duration::from_secs(5)).ok()
    }
    /// Stops listening and ends every connection.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.tx.send(Event::Stop);
        // wake the accept loop
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(200));
    }
}

/// Starts a session server listening on `bind` (port 0 picks a free one; the address is in the handle).
pub fn start(cfg: Config, bind: SocketAddr) -> std::io::Result<LiveHandle> {
    let session = Session::new(cfg).ok_or_else(|| std::io::Error::other("no randomness for the session"))?;
    start_with(session, bind)
}

/// A session server carrying on from another host's `export_state()` (host handoff).
pub fn start_from(cfg: Config, bind: SocketAddr, state: &Value) -> std::io::Result<LiveHandle> {
    let session = Session::import(cfg, state, 0).map_err(std::io::Error::other)?;
    start_with(session, bind)
}

fn start_with(session: Session, bind: SocketAddr) -> std::io::Result<LiveHandle> {
    let listener = TcpListener::bind(bind)?;
    let addr = listener.local_addr()?;
    let (tx, rx) = channel();
    let h = LiveHandle {
        addr,
        session_id: session.id.clone(),
        tx: tx.clone(),
        next_conn: Arc::new(AtomicU64::new(1)),
        stop: Arc::new(AtomicBool::new(false)),
    };
    std::thread::Builder::new().name("cc-live-session".into()).spawn(move || run_session(session, rx))?;
    let h2 = h.clone();
    std::thread::Builder::new().name("cc-live-accept".into()).spawn(move || accept_loop(listener, h2))?;
    Ok(h)
}

fn accept_loop(listener: TcpListener, h: LiveHandle) {
    let open = Arc::new(AtomicU64::new(0));
    for stream in listener.incoming() {
        if h.stop.load(Ordering::SeqCst) {
            break;
        }
        let Ok(stream) = stream else { continue };
        if usize::try_from(open.load(Ordering::SeqCst)).unwrap_or(usize::MAX) >= MAX_CONNS {
            drop(stream);
            continue;
        }
        let (h, open) = (h.clone(), open.clone());
        open.fetch_add(1, Ordering::SeqCst);
        let _ = std::thread::Builder::new().name("cc-live-conn".into()).spawn(move || {
            serve_conn(stream, &h);
            open.fetch_sub(1, Ordering::SeqCst);
        });
    }
}

fn serve_conn(mut stream: TcpStream, h: &LiveHandle) {
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let local = stream.peer_addr().is_ok_and(|p| p.ip().is_loopback());
    let Ok(head) = ws::read_head(&mut stream) else { return };
    if ws::accept(&mut stream, &head).is_err() {
        return;
    }
    let _ = stream.set_read_timeout(Some(IDLE));
    let Ok(writer_stream) = stream.try_clone() else { return };
    let conn = h.next_conn.fetch_add(1, Ordering::Relaxed);
    let (wtx, wrx) = channel::<Wire>();
    if h.tx.send(Event::Open(conn, wtx.clone(), false, "lan")).is_err() {
        return;
    }
    let writer = std::thread::Builder::new().name("cc-live-write".into()).spawn(move || write_loop(writer_stream, &wrx));
    let max = if local { MAX_FRAME_LOCAL } else { MAX_FRAME };
    loop {
        match ws::read_msg(&mut stream, max, true) {
            Ok(ws::Msg::Binary(b)) => {
                if h.tx.send(Event::Frame(conn, b)).is_err() {
                    break;
                }
            }
            Ok(ws::Msg::Ping(p)) => {
                let _ = wtx.send(Wire::Bytes(pong_marker(&p)));
            }
            Ok(ws::Msg::Text | ws::Msg::Pong) => {}
            Err(_) => break,
        }
    }
    let _ = h.tx.send(Event::Closed(conn));
    let _ = wtx.send(Wire::Close("closed".into()));
    let _ = stream.shutdown(std::net::Shutdown::Both);
    if let Ok(w) = writer {
        let _ = w.join();
    }
}

/// A WebSocket pong travels through the writer as bytes with this prefix (never a valid sealed frame or envelope: a
/// frame starts with version 1, an envelope with a map byte).
const PONG: u8 = 0xff;
fn pong_marker(p: &[u8]) -> Vec<u8> {
    let mut v = vec![PONG];
    v.extend_from_slice(p.get(..p.len().min(125)).unwrap_or(&[]));
    v
}

fn write_loop(mut s: TcpStream, rx: &Receiver<Wire>) {
    while let Ok(w) = rx.recv() {
        match w {
            Wire::Bytes(b) if b.first() == Some(&PONG) => {
                if ws::write_pong(&mut s, b.get(1..).unwrap_or(&[]), None).is_err() {
                    break;
                }
            }
            Wire::Bytes(b) => {
                if ws::write_binary(&mut s, &b, None).is_err() {
                    break;
                }
            }
            Wire::Close(why) => {
                let _ = ws::write_close(&mut s, 1000, &why, None);
                let _ = s.flush();
                let _ = s.shutdown(std::net::Shutdown::Both);
                break;
            }
        }
    }
}

struct ConnIo {
    w: Sender<Wire>,
    keys: Option<crypto::Session>,
    local: bool,
}

fn run_session(mut s: Session, rx: Receiver<Event>) {
    let t0 = Instant::now();
    let now = || u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX);
    let mut conns: HashMap<ConnId, ConnIo> = HashMap::new();
    let mut last_tick = Instant::now();
    loop {
        let out = match rx.recv_timeout(TICK) {
            Err(RecvTimeoutError::Disconnected) | Ok(Event::Stop) => break,
            Err(RecvTimeoutError::Timeout) => Vec::new(),
            Ok(Event::Open(c, w, local, via)) => {
                conns.insert(c, ConnIo { w, keys: None, local });
                s.open(c, local, via, now());
                Vec::new()
            }
            Ok(Event::Frame(c, b) | Event::Local(c, b)) => {
                let body = match conns.get_mut(&c) {
                    None => continue,
                    Some(io) => match io.keys.as_mut() {
                        None => Ok(Some(b)),
                        Some(k) => match k.open(&b) {
                            Ok(crypto::Opened::Data(body)) => Ok(Some(body)),
                            Ok(crypto::Opened::Rekeyed) => Ok(None),
                            Err(e) => Err(e.to_string()),
                        },
                    },
                };
                match body.map(|b| b.map(|b| Envelope::decode(&b))) {
                    Ok(None) => Vec::new(),
                    Ok(Some(Ok(env))) => s.frame(c, env, now()),
                    Ok(Some(Err(why))) => {
                        if let Some(io) = conns.remove(&c) {
                            let _ = io.w.send(Wire::Close(why.to_string()));
                        }
                        s.closed(c, now())
                    }
                    Err(why) => {
                        if let Some(io) = conns.remove(&c) {
                            let _ = io.w.send(Wire::Close(why));
                        }
                        s.closed(c, now())
                    }
                }
            }
            Ok(Event::Closed(c)) => {
                conns.remove(&c);
                s.closed(c, now())
            }
            Ok(Event::Stats(reply)) => {
                let _ = reply.send((s.stats.clone(), s.seq(), conns.len()));
                Vec::new()
            }
            Ok(Event::Export(reply)) => {
                let _ = reply.send(s.export());
                Vec::new()
            }
        };
        let mut out = out;
        if last_tick.elapsed() >= TICK {
            last_tick = Instant::now();
            out.extend(s.tick(now()));
        }
        for o in out {
            match o {
                Out::To(c, env) => {
                    let Some(io) = conns.get_mut(&c) else { continue };
                    let Ok(bytes) = env.encode() else { continue };
                    let frame = match io.keys.as_mut() {
                        None => Ok(bytes),
                        Some(k) => k.seal(&bytes),
                    };
                    match frame {
                        Ok(f) => {
                            let _ = io.w.send(Wire::Bytes(f));
                        }
                        Err(e) => {
                            let _ = io.w.send(Wire::Close(e.to_string()));
                        }
                    }
                }
                Out::Secure(c, keys) => {
                    if let Some(io) = conns.get_mut(&c)
                        && !io.local
                    {
                        io.keys = Some(*keys);
                    }
                }
                Out::Rekey(c) => {
                    if let Some(io) = conns.get_mut(&c)
                        && let Some(k) = io.keys.as_mut()
                    {
                        match k.rekey() {
                            Ok(f) => {
                                let _ = io.w.send(Wire::Bytes(f));
                            }
                            Err(e) => {
                                let _ = io.w.send(Wire::Close(e.to_string()));
                            }
                        }
                    }
                }
                Out::Close(c, why) => {
                    if let Some(io) = conns.remove(&c) {
                        let _ = io.w.send(Wire::Close(why));
                    }
                }
            }
        }
    }
    for (_, io) in conns.drain() {
        let _ = io.w.send(Wire::Close("stopped".into()));
    }
}

// -------------------------------------------------------------------------------------------------------------------
// The joiner's side

/// How a joiner gets in: a short code (`TT-SSSSS`), a rejoin ticket (`h:<c>`, `rejoin:<session>`), or nothing (a
/// plain test server).
#[derive(Clone)]
pub enum Secret {
    Plain,
    Code(String),
    Key { kid: String, secret: Vec<u8>, context: String },
}

/// One joiner's connection: envelopes in and out, sealed after the handshake.
pub struct Joiner {
    stream: TcpStream,
    keys: Option<crypto::Session>,
    /// `exporter("ticket")` of the sealed connection (the rejoin ticket), empty when plain.
    pub ticket: Vec<u8>,
    /// `exporter("assertion")` as base64url (a signed-in name's audience), empty when plain.
    pub aud: String,
    max: usize,
}

fn mask() -> [u8; 4] {
    let r = crypto::random(4).unwrap_or_default();
    let mut m = [0u8; 4];
    for (d, s) in m.iter_mut().zip(r) {
        *d = s;
    }
    m
}

impl Joiner {
    /// Connects to `addr` (`host:port`), runs the handshake for `secret`, ready for `hello`.
    pub fn connect(addr: &str, secret: &Secret) -> Result<Joiner, String> {
        let sa: SocketAddr = std::net::ToSocketAddrs::to_socket_addrs(addr)
            .map_err(|e| e.to_string())?
            .next()
            .ok_or_else(|| format!("no address for {addr}"))?;
        // § 8: a connection attempt that hasn't opened after 1.5 s is abandoned
        let mut stream = TcpStream::connect_timeout(&sa, Duration::from_millis(1500)).map_err(|e| e.to_string())?;
        let _ = stream.set_nodelay(true);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
        let key: [u8; 16] = {
            let r = crypto::random(16).ok_or("no randomness")?;
            let mut k = [0u8; 16];
            for (d, s) in k.iter_mut().zip(r) {
                *d = s;
            }
            k
        };
        ws::connect(&mut stream, addr, &key).map_err(|e| e.to_string())?;
        let local = sa.ip().is_loopback();
        let mut j = Joiner { stream, keys: None, ticket: vec![], aud: String::new(), max: if local { MAX_FRAME_LOCAL } else { MAX_FRAME } };
        let (kid, secret, context) = match secret {
            Secret::Plain => return Ok(j),
            Secret::Code(code) => {
                let (tag, sec) = crypto::parse_code(code).ok_or("that is not a session code")?;
                (tag.clone(), sec, format!("lan:{tag}"))
            }
            Secret::Key { kid, secret, context } => (kid.clone(), secret.clone(), context.clone()),
        };
        let (mut hs, a) = crypto::JoinerHandshake::start(&secret, &context);
        j.send_raw(&Envelope::new("pake").body(Value::map(vec![("kid", kid.as_str().into()), ("m", Value::Bin(a))])))?;
        let b = j.recv_wait()?;
        let bm = b.b.get("m").and_then(|v| if let Value::Bin(x) = v { Some(x.clone()) } else { None }).ok_or("bad handshake")?;
        let c = hs.receive(&bm).map_err(|e| e.to_string())?;
        j.send_raw(&Envelope::new("pake").body(Value::map(vec![("m", Value::Bin(c))])))?;
        let d = j.recv_wait().map_err(|_| "wrong code".to_string())?;
        let dm = d.b.get("m").and_then(|v| if let Value::Bin(x) = v { Some(x.clone()) } else { None }).ok_or("bad handshake")?;
        let session = hs.finish(&dm).map_err(|e| e.to_string())?;
        j.ticket = session.exporter("ticket").to_vec();
        j.aud = crypto::b64url(&session.exporter("assertion"));
        j.keys = Some(session);
        Ok(j)
    }

    fn send_raw(&mut self, env: &Envelope) -> Result<(), String> {
        let b = env.encode().map_err(|e| e.0)?;
        ws::write_binary(&mut self.stream, &b, Some(mask())).map_err(|e| e.to_string())
    }

    /// The next envelope, waiting at most the socket's read timeout for it to start (None: nothing came, or a control
    /// message was handled).
    fn recv_raw(&mut self) -> Result<Option<Envelope>, String> {
        match ws::read_msg(&mut self.stream, self.max, false) {
            Ok(ws::Msg::Binary(b)) => {
                let body = match self.keys.as_mut() {
                    None => b,
                    Some(k) => match k.open(&b).map_err(|e| e.to_string())? {
                        crypto::Opened::Data(d) => d,
                        crypto::Opened::Rekeyed => return Ok(None),
                    },
                };
                Envelope::decode(&body).map(Some).map_err(|e| e.0.to_string())
            }
            Ok(ws::Msg::Ping(p)) => {
                let _ = ws::write_pong(&mut self.stream, &p, Some(mask()));
                Ok(None)
            }
            Ok(ws::Msg::Text | ws::Msg::Pong) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    /// The handshake's next envelope (10 s at most).
    fn recv_wait(&mut self) -> Result<Envelope, String> {
        let t0 = Instant::now();
        loop {
            if let Some(e) = self.recv(Duration::from_millis(200))? {
                return Ok(e);
            }
            if t0.elapsed() > Duration::from_secs(10) {
                return Err("the session did not answer".into());
            }
        }
    }

    /// Sends one envelope (sealed after the handshake).
    pub fn send(&mut self, env: &Envelope) -> Result<(), String> {
        let b = env.encode().map_err(|e| e.0)?;
        let f = match self.keys.as_mut() {
            None => b,
            Some(k) => k.seal(&b).map_err(|e| e.to_string())?,
        };
        ws::write_binary(&mut self.stream, &f, Some(mask())).map_err(|e| e.to_string())
    }

    /// The next envelope within `wait` (None: nothing yet). An error means the connection is over. Only the wait for
    /// a message to start is bounded by `wait`: once its first byte is there the whole message is read (10 s at most),
    /// so a slow frame never leaves the stream in the middle of one.
    pub fn recv(&mut self, wait: Duration) -> Result<Option<Envelope>, String> {
        let _ = self.stream.set_read_timeout(Some(wait.max(Duration::from_millis(1))));
        let mut one = [0u8; 1];
        match self.stream.peek(&mut one) {
            Ok(0) => return Err("closed".into()),
            Ok(_) => {}
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => return Ok(None),
            Err(e) => return Err(e.to_string()),
        }
        let _ = self.stream.set_read_timeout(Some(Duration::from_secs(10)));
        self.recv_raw()
    }

    /// Closes the connection.
    pub fn close(&mut self) {
        let _ = ws::write_close(&mut self.stream, 1000, "bye", Some(mask()));
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}
