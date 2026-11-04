//! `cadcraft-cli live-server`: Studio Live's session server without a window, with the flags of 0.5's `live-server`
//! (tools/live-test/server), so the 0.5 test suites (live-client.js over real sockets) run against it:
//!
//!   cadcraft-cli live-server [--bind 127.0.0.1:0] [--app cadcraft] [--auto-approve] [--token <local token>]
//!                            [--log-keep N] [--max-clients N] [--plain] [--from <state file>]
//!
//! Prints one JSON line `{"addr": "...", "session": "..."}` when listening, then serves until stdin ends. `--plain` lets loopback
//! test clients skip the handshake and frame encryption (tests only; the app never sets it). It binds where it is told:
//! 127.0.0.1 unless the caller names the LAN.

use std::io::BufRead;
use std::process::ExitCode;

pub fn run(args: &[&str]) -> ExitCode {
    let val = |k: &str| args.iter().position(|a| *a == k).and_then(|i| args.get(i + 1)).map(|s| s.to_string());
    let flag = |k: &str| args.contains(&k);
    let mut cfg = cc_live::server::Config {
        app: val("--app").unwrap_or_default(),
        auto_approve: flag("--auto-approve"),
        plain: flag("--plain"),
        local_token: val("--token").unwrap_or_default(),
        ..Default::default()
    };
    if let Some(n) = val("--log-keep").and_then(|v| v.parse().ok()) {
        cfg.log_keep = n;
    }
    if let Some(n) = val("--max-clients").and_then(|v| v.parse().ok()) {
        cfg.max_clients = n;
    }
    let Ok(bind) = val("--bind").unwrap_or_else(|| "127.0.0.1:0".into()).parse::<std::net::SocketAddr>() else {
        eprintln!("live-server: --bind host:port");
        return ExitCode::from(2);
    };
    let started = match val("--from") {
        Some(f) => {
            let state = std::fs::read(&f).ok().filter(|b| b.len() <= 256 << 20).and_then(|b| cc_live::msgpack::unpack(&b).ok());
            match state {
                Some(st) => cc_live::net::start_from(cfg, bind, &st),
                None => {
                    eprintln!("live-server: --from: not a MessagePack state file");
                    return ExitCode::from(2);
                }
            }
        }
        None => cc_live::net::start(cfg, bind),
    };
    let h = match started {
        Ok(h) => h,
        Err(e) => {
            eprintln!("live-server: {e}");
            return ExitCode::from(1);
        }
    };
    println!("{{\"addr\":\"{}\",\"session\":\"{}\"}}", h.addr, h.session_id);
    let stdin = std::io::stdin();
    for _ in stdin.lock().lines() {}
    h.stop();
    ExitCode::SUCCESS
}
