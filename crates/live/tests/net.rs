//! Studio Live end to end between Rust instances (M16.9): the session server (`cc_live::net`) on a loopback port and
//! joiners (`cc_live::net::Joiner` + the client core) over real WebSockets. Every shared scenario of
//! tools/live-test/vectors.json runs twice: plain (as 0.5's test-scenarios.mjs does) and sealed (a short code, the
//! SPAKE2 handshake, ChaCha20-Poly1305 frames). The same scenarios run from the 0.5 JS client against this server in
//! the harness (forks/tools/live-detailfin), and from these joiners against 0.5's own server.
#![cfg(not(target_arch = "wasm32"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use cc_live::client::{Client, Event};
use cc_live::envelope::Envelope;
use cc_live::msgpack::Value;
use cc_live::net::{Joiner, Secret};
use cc_live::server::Config;
use cc_live::session::Touch;
use serde_json::Value as J;

const TOKEN: &str = "net-test-token";

fn vectors() -> J {
    serde_json::from_str(include_str!("vectors.json")).unwrap()
}

/// One joiner: its socket, the client core, what it has seen.
struct Peer {
    j: Joiner,
    core: Client,
    refusals: Vec<(u64, String, Option<u64>)>,
    commits: Vec<(u64, u64)>,
    locks: HashSet<String>,
    roles: HashMap<u64, String>,
    codes: Vec<String>,
    live: bool,
}

impl Peer {
    fn join(addr: &str, secret: &Secret, host: bool) -> Peer {
        let mut j = Joiner::connect(addr, secret).expect("connect");
        let core = Client::new();
        let mut hello = core.hello("cadcraft", 1, if host { "H" } else { "guest" });
        if host && let Value::Map(m) = &mut hello.b {
            m.push(("role".into(), Value::from("host")));
            m.push(("token".into(), Value::from(TOKEN)));
        }
        j.send(&hello).expect("hello");
        let mut p =
            Peer { j, core, refusals: vec![], commits: vec![], locks: HashSet::new(), roles: HashMap::new(), codes: vec![], live: false };
        p.until(|p| p.live, "welcome");
        p
    }

    /// Reads what is waiting (a few ms at most).
    fn pump(&mut self) {
        while let Ok(Some(env)) = self.j.recv(Duration::from_millis(2)) {
            match env.t.as_str() {
                "ping" => {
                    let mut e = Envelope::new("pong");
                    e.n = env.n;
                    let _ = self.j.send(&e);
                }
                "locks" => {
                    for k in env.b.get("add").map(Value::as_arr).unwrap_or(&[]) {
                        self.locks.insert(k.as_str().unwrap_or("").to_string());
                    }
                    for k in env.b.get("del").map(Value::as_arr).unwrap_or(&[]) {
                        self.locks.remove(k.as_str().unwrap_or(""));
                    }
                }
                "peer" => {
                    if let (Some(c), Some(r)) = (env.b.get("c").and_then(Value::as_u64), env.b.get("role").and_then(Value::as_str)) {
                        self.roles.insert(c, r.to_string());
                    }
                }
                "code" => self.codes.push(env.b.get("code").and_then(Value::as_str).unwrap_or("").to_string()),
                _ => {}
            }
            for ev in self.core.receive(&env) {
                match ev {
                    Event::Welcome { .. } => self.live = true,
                    Event::Refused { n, reason, by, .. } => self.refusals.push((n, reason, by)),
                    Event::Confirmed { n, q } => self.commits.push((n, q)),
                    _ => {}
                }
            }
        }
    }

    fn until(&mut self, f: impl Fn(&Peer) -> bool, what: &str) {
        let t0 = Instant::now();
        while !f(self) {
            assert!(t0.elapsed() < Duration::from_secs(10), "timed out: {what}");
            self.pump();
        }
    }
}

fn touches(t: &J) -> Vec<Touch> {
    t.as_array()
        .unwrap()
        .iter()
        .map(|x| Touch {
            k: x["k"].as_str().unwrap().into(),
            m: x["m"].as_str().unwrap().chars().next().unwrap(),
            f: x["f"].as_str().map(String::from),
            r: x["r"].as_u64(),
            g: x["g"].as_u64().unwrap_or(0) > 0,
        })
        .collect()
}

fn strs(v: &J) -> Vec<String> {
    v.as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect()
}

fn keys(env: &str, ks: &[String]) -> Envelope {
    Envelope::new(env).body(Value::map(vec![("keys", Value::Arr(ks.iter().map(|k| Value::from(k.as_str())).collect()))]))
}

/// A session server to run against: ours in-process, or another program with 0.5 `live-server`'s flags
/// (`LIVE_SERVER=…/live-server`: 0.5's own, built from desktop-shell v0.5.0, as an oracle).
enum Srv {
    Ours(cc_live::net::LiveHandle),
    Other(std::process::Child),
}

fn server(sealed: bool) -> (Srv, String) {
    if let Ok(bin) = std::env::var("LIVE_SERVER") {
        use std::io::BufRead;
        let mut args = vec!["--bind", "127.0.0.1:0", "--app", "cadcraft", "--auto-approve", "--token", TOKEN];
        if !sealed {
            args.push("--plain");
        }
        let mut child = std::process::Command::new(bin)
            .args(&args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("live-server");
        let mut line = String::new();
        std::io::BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
        let v: J = serde_json::from_str(&line).unwrap();
        let addr = v["addr"].as_str().unwrap().to_string();
        return (Srv::Other(child), addr);
    }
    let cfg = Config { app: "cadcraft".into(), auto_approve: true, plain: !sealed, local_token: TOKEN.into(), ..Default::default() };
    let h = cc_live::net::start(cfg, "127.0.0.1:0".parse().unwrap()).expect("server");
    let addr = h.addr.to_string();
    (Srv::Ours(h), addr)
}

fn stop(s: Srv) {
    match s {
        Srv::Ours(h) => h.stop(),
        Srv::Other(mut c) => {
            drop(c.stdin.take());
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn run_scenario(sc: &J, sealed: bool) {
    let (h, addr) = server(sealed);
    let names = strs(&sc["clients"]);
    let mut peers: HashMap<String, Peer> = HashMap::new();
    let mut code = None;
    for name in &names {
        let host = name == "H";
        // the host comes in with the local token (no handshake); the others with the code it made
        let secret = match (&code, sealed && !host) {
            (Some(c), true) => Secret::Code(String::clone(c)),
            _ => Secret::Plain,
        };
        let mut p = Peer::join(&addr, &secret, host);
        if host && sealed {
            p.j.send(&Envelope::new("invite")).unwrap();
            p.until(|p| !p.codes.is_empty(), "code");
            code = p.codes.first().cloned();
        }
        peers.insert(name.clone(), p);
    }
    let id = |peers: &HashMap<String, Peer>, n: &str| peers[n].core.c;
    for (i, st) in sc["steps"].as_array().unwrap().iter().enumerate() {
        let at = format!("{} — step {}", sc["name"], i + 1);
        let by = st["by"].as_str().unwrap().to_string();
        if !st["touched"].is_null() {
            let p = peers.get_mut(&by).unwrap();
            let (r0, c0) = (p.refusals.len(), p.commits.len());
            let env = p.core.edit(st["label"].as_str().unwrap_or("edit"), vec![], touches(&st["touched"]));
            p.j.send(&env).unwrap();
            p.until(|p| p.refusals.len() > r0 || p.commits.len() > c0, &at);
            if let Some(q) = st["expect"]["commit"].as_u64() {
                assert_eq!(p.refusals.len(), r0, "{at}: refused {:?}", p.refusals.last());
                assert_eq!(p.commits.last().map(|x| x.1), Some(q), "{at}: seq");
            } else {
                let (_, reason, rby) = p.refusals.last().cloned().unwrap();
                assert_eq!(reason, st["expect"]["refuse"].as_str().unwrap(), "{at}: reason");
                if let Some(e) = st["expect"]["by"].as_str() {
                    let want = id(&peers, e);
                    assert_eq!(rby, Some(want), "{at}: by");
                }
            }
        } else if !st["hold"].is_null() || !st["release"].is_null() {
            let hold = !st["hold"].is_null();
            let ks = strs(if hold { &st["hold"] } else { &st["release"] });
            let who = id(&peers, &by);
            peers.get_mut(&by).unwrap().j.send(&keys(if hold { "hold" } else { "release" }, &ks)).unwrap();
            for p in peers.values_mut() {
                p.until(|p| ks.iter().all(|k| if hold { p.core.holds.get(k) == Some(&who) } else { !p.core.holds.contains_key(k) }), &at);
            }
        } else if !st["lock"].is_null() || !st["unlock"].is_null() {
            let on = !st["lock"].is_null();
            let ks = strs(if on { &st["lock"] } else { &st["unlock"] });
            peers.get_mut(&by).unwrap().j.send(&keys(if on { "lock" } else { "unlock" }, &ks)).unwrap();
            for p in peers.values_mut() {
                p.until(|p| ks.iter().all(|k| p.locks.contains(k) == on), &at);
            }
        } else if !st["role"].is_null() {
            let who = id(&peers, st["role"]["who"].as_str().unwrap());
            let role = st["role"]["role"].as_str().unwrap().to_string();
            let e = Envelope::new("role").body(Value::map(vec![("c", Value::from(who)), ("role", Value::from(role.as_str()))]));
            peers.get_mut(&by).unwrap().j.send(&e).unwrap();
            for p in peers.values_mut() {
                p.until(|p| p.core.c == who || p.roles.get(&who) == Some(&role), &at);
            }
        }
    }
    for p in peers.values_mut() {
        p.j.close();
    }
    stop(h);
}

#[test]
fn the_shared_scenarios_pass_over_sockets_plain_and_sealed() {
    let v = vectors();
    let scs = v["scenarios"].as_array().unwrap();
    assert!(scs.len() >= 11);
    for sc in scs {
        run_scenario(sc, false);
        run_scenario(sc, true);
    }
}

#[test]
fn a_wrong_code_never_gets_in_and_three_replace_the_code() {
    let cfg = Config { app: "cadcraft".into(), auto_approve: true, local_token: TOKEN.into(), ..Default::default() };
    let h = cc_live::net::start(cfg, "127.0.0.1:0".parse().unwrap()).expect("server");
    let addr = h.addr.to_string();
    let mut host = Peer::join(&addr, &Secret::Plain, true);
    host.j.send(&Envelope::new("invite")).unwrap();
    host.until(|p| !p.codes.is_empty(), "code");
    let code = host.codes[0].clone();
    // the same tag, another secret
    let wrong = format!("{}-{}", &code[..2], if &code[3..] == "00000" { "11111" } else { "00000" });
    for _ in 0..3 {
        assert!(Joiner::connect(&addr, &Secret::Code(wrong.clone())).is_err());
    }
    // three wrong tries: the host gets a new code, and the old one no longer works
    host.until(|p| p.codes.len() == 2, "replaced code");
    assert!(Joiner::connect(&addr, &Secret::Code(code)).is_err());
    let fresh = host.codes[1].clone();
    let a = Peer::join(&addr, &Secret::Code(fresh), false);
    assert!(a.live);
    // a plain hello on a sealed server is closed: the handshake comes first
    let mut p = Joiner::connect(&addr, &Secret::Plain).unwrap();
    p.send(&Client::new().hello("cadcraft", 1, "x")).unwrap();
    let mut closed = false;
    for _ in 0..200 {
        match p.recv(Duration::from_millis(10)) {
            Err(_) => {
                closed = true;
                break;
            }
            Ok(Some(e)) => assert_ne!(e.t, "welcome"),
            Ok(None) => {}
        }
    }
    assert!(closed, "a hello before the handshake closes the connection");
    h.stop();
}

#[test]
fn hostile_bytes_close_only_that_connection() {
    use std::io::Write;
    let cfg = Config { app: "cadcraft".into(), auto_approve: true, plain: true, local_token: TOKEN.into(), ..Default::default() };
    let h = cc_live::net::start(cfg, "127.0.0.1:0".parse().unwrap()).expect("server");
    let addr = h.addr.to_string();
    let mut host = Peer::join(&addr, &Secret::Plain, true);
    // not a websocket at all; an http request for a file; garbage after the upgrade
    for junk in [&b"\x00\x01\x02garbage\r\n\r\n"[..], b"GET /etc/passwd HTTP/1.1\r\n\r\n", b"GET / HTTP/1.1\r\nHost: x\r\n\r\n"] {
        let mut s = std::net::TcpStream::connect(&addr).unwrap();
        let _ = s.write_all(junk);
    }
    let mut j = Joiner::connect(&addr, &Secret::Plain).unwrap();
    // an envelope that does not decode
    j.send(&Envelope::new("hello")).unwrap();
    let mut s = std::net::TcpStream::connect(&addr).unwrap();
    let _ = s.write_all(b"GET /live HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n");
    let _ = s.write_all(&[0x82, 0x85, 1, 2, 3, 4, 0xc1, 0xc1, 0xc1, 0xc1, 0xc1]);
    // the session is still there for the host
    let env = host.core.edit("still here", vec![], vec![Touch { k: "e:1".into(), m: 'c', f: None, r: None, g: false }]);
    host.j.send(&env).unwrap();
    host.until(|p| !p.commits.is_empty(), "the host's edit commits");
    h.stop();
}
