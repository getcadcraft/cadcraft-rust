//! Studio Live in an app window, the part every app shares (0.5: Studio's glue `studio_live.rs`, `host.live`, and each
//! app's live.js `createLiveAdapter` / `messages`): the session this window hosts or joined, its messages carried into the
//! app's adapter every frame, the app's own steps sent out, per-person undo, the host's approve cards (with a signed-in
//! name's tick, checked offline: cc_live::assertion), discovery on the LAN (cc_live::mdns, `_ccstudio._tcp`), the join
//! QR, and the Live window. Each app supplies a [`LiveView`]: its adapter (live.js `createLive`) and where its messages go.
//!
//! Commands, for every app with a prefix `p` (`detail`, `part`, `cad` …): `p.live.host` = `[bind address]` (default the
//! LAN on a free port), `p.live.join` = `<host:port> <code | plain>` or `<code>` (found on the LAN by its tag) or a join
//! link `http://<host>:<port>/apps/<slug>/#live=<code>`, `p.live.approve` = `<c> [editor|viewer]`, `p.live.deny` = `<c>`,
//! `p.live.invite`, `p.live.leave`, `p.live.window` = `open | close`, `p.live.pump` = `<ms> [wait] [approve] [live]`
//! (headless: carry messages until everything sent is decided, at most that long; `wait` the whole time; `live` until
//! this window is in (let in, caught up); `approve` lets every joiner in as an editor meanwhile).

#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::{Receiver, channel};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

use cc_live::doc::Adapter;
#[cfg(not(target_arch = "wasm32"))]
use cc_live::doc::{DocEvent, LiveDoc};
#[cfg(not(target_arch = "wasm32"))]
use cc_live::envelope::Envelope;
use cc_live::msgpack::Value as Mp;

/// One app window's side of Live: implemented by each app's view.
pub trait LiveView {
    /// the app's slug in the session (`hello.app`): "cadcraft", "partcraft" …
    const APP: &'static str;
    /// the command prefix: "cad", "part" …
    const PREFIX: &'static str;
    fn live_slot(&mut self) -> &mut Option<LiveSession>;
    fn live_window_open(&mut self) -> &mut bool;
    /// Run `f` with the window's adapter (client number `c`: new ids come from its range).
    fn with_adapter(&mut self, c: u64, f: &mut dyn FnMut(&mut dyn Adapter));
    /// A welcome (first or after a rejoin): new objects number from client c's range (live.js `claimIds`).
    fn claim_ids(&mut self, c: u64);
    /// The session is over (live.js `unbind`): numbering goes on above every id (`releaseIds`), the model's own undo empty.
    fn release_ids(&mut self);
    /// Joined or hosting: the model's own undo / redo stacks start empty (the session's are per person).
    fn live_bind(&mut self);
    /// A value that changes when the model does (its version), to know when others' edits landed.
    fn model_version(&self) -> u64;
    /// Others' edits landed: rebuild what the window derives from the model.
    fn after_remote(&mut self);
    /// The app's words (its 0.5 catalog's `live.*` keys).
    fn tr(&self, key: &str, vars: &[(&str, String)]) -> String;
    /// Show a message on the app's status line.
    fn say(&mut self, msg: String);
    /// The app's own steps since the last call (live.js's wrapped `change` / `step`): (label, ops, inverse), each with
    /// its `{o:'g'}` mark from `g()`. Apps that send their steps themselves (DetailCraft: `live_local`) keep the default.
    fn local_steps(&mut self, _g: &mut dyn FnMut() -> String) -> Vec<(String, Mp, Mp)> {
        vec![]
    }
    /// The shared document as live.js `hash()` sees it (what the convergence checks compare), as JSON.
    fn live_state(&mut self) -> Option<serde_json::Value> {
        None
    }
    /// The hosting UI's own words (Studio's, not in the 0.5 app catalogs): plain English, listed in docs/gaps.md.
    fn label_live_window(&self) -> String {
        "Live session".into()
    }
}

/// A joiner asking in, on the host's approve card.
#[derive(Debug, Clone)]
pub struct Knock {
    pub c: u64,
    pub name: String,
    pub device: String,
    pub via: String,
    /// a signed-in name that checked out (cc_live::assertion): the tick
    pub signed: Option<cc_live::assertion::Signed>,
    /// why an assertion that came with the joiner was refused ("format", "expired" …)
    pub refused: Option<&'static str>,
    /// the joiner's picture, when it matches their signed-in name
    pub picture: Vec<u8>,
}

/// The browser edition joins through its own WebSocket (not built yet): no session in a window there.
#[cfg(target_arch = "wasm32")]
pub struct LiveSession {
    pub doc: cc_live::doc::LiveDoc,
    pub app: &'static str,
}

/// How this window talks to the session.
#[cfg(not(target_arch = "wasm32"))]
pub enum Link {
    /// the host's own window, in-process with the server it started
    Local(cc_live::net::LocalClient),
    /// a joiner's connection; envelopes arrive through a reader thread
    Remote { tx: std::sync::Arc<std::sync::Mutex<cc_live::net::Joiner>>, rx: Receiver<Result<Envelope, String>> },
}

/// A live session in this window.
#[cfg(not(target_arch = "wasm32"))]
pub struct LiveSession {
    pub doc: LiveDoc,
    pub link: Link,
    pub server: Option<cc_live::net::LiveHandle>,
    pub addr: String,
    pub code: String,
    /// joiners waiting for the host
    pub knocks: Vec<Knock>,
    /// this machine's LAN addresses (the QR and the join link)
    pub lan: Vec<String>,
    /// the mDNS advertisement of the session's tag (hosts)
    pub advert: Option<cc_live::mdns::Advert>,
    pub app: &'static str,
    g: u64,
    closed: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for LiveSession {
    fn drop(&mut self) {
        if let Some(a) = self.advert.take() {
            a.stop();
        }
        if let Some(h) = &self.server {
            h.stop();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl LiveSession {
    pub fn send(&mut self, env: &Envelope) {
        match &self.link {
            Link::Local(l) => l.send(env),
            Link::Remote { tx, .. } => {
                let r = tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).send(env);
                if r.is_err() {
                    self.closed = true;
                }
            }
        }
    }
    pub fn flush_out(&mut self) {
        let out = std::mem::take(&mut self.doc.outbox);
        for e in &out {
            self.send(e);
        }
    }
    pub fn is_host(&self) -> bool {
        self.server.is_some()
    }
    /// The next step's group mark (`{o:'g', g:'s<n>'}`): one Ctrl+Z undoes it whole.
    pub fn next_group(&mut self) -> String {
        self.g = self.g.saturating_add(1);
        format!("s{}", self.g)
    }
    /// The joiner's link (and the QR): `http://<host>:<port>/apps/<slug>/#live=<code>` (LIVE-PROTOCOL § 12).
    pub fn join_link(&self) -> Option<String> {
        if self.code.is_empty() || !self.is_host() {
            return None;
        }
        let port = self.addr.rsplit_once(':').map_or("", |(_, p)| p);
        let host = self.lan.first().cloned().unwrap_or_else(|| self.addr.rsplit_once(':').map_or(String::new(), |(h, _)| h.to_string()));
        Some(format!("http://{host}:{port}/apps/{}/#live={}", self.app, self.code))
    }
}

/// The status line while live (plain English: Studio's own words, not in the 0.5 app catalogs).
#[cfg(not(target_arch = "wasm32"))]
pub fn hosting_line(s: &LiveSession) -> String {
    let people = s.doc.peers.values().filter(|p| p.state != "gone" && p.state != "removed").count();
    if s.server.is_some() {
        // a session open to the LAN is reached at this machine's address, not 0.0.0.0
        let addr = match (s.addr.strip_prefix("0.0.0.0:"), s.lan.first()) {
            (Some(port), Some(ip)) => format!("{ip}:{port}"),
            _ => s.addr.clone(),
        };
        format!("Live: hosting at {addr} · code {} · {people} in", if s.code.is_empty() { "…" } else { &s.code })
    } else {
        format!("Live: joined {} · {} others in", s.addr, people)
    }
}

/// A joiner's signed-in name, checked as 0.5's approve card does (verifyAssertion with the connection's `aud`, then
/// pictureMatches): (the tick, why refused, the picture when it matches).
pub fn check_assertion(assertion: &str, aud: &str, picture: &[u8]) -> (Option<cc_live::assertion::Signed>, Option<&'static str>, Vec<u8>) {
    if assertion.is_empty() {
        return (None, None, vec![]);
    }
    let now = cc_clock::now_ms() / 1000.0;
    match cc_live::assertion::verify_assertion(assertion, aud, now, cc_live::assertion::ASSERTION_KEYS) {
        Ok(s) => {
            let pic = match &s.pic {
                Some(p) if cc_live::assertion::picture_matches(picture, p) => picture.to_vec(),
                _ => vec![],
            };
            (Some(s), None, pic)
        }
        Err(e) => (None, Some(e.as_str()), vec![]),
    }
}

/// Carry what arrived from the session into the model and send what the session waits for (every frame).
#[cfg(not(target_arch = "wasm32"))]
pub fn poll<V: LiveView>(v: &mut V) {
    let Some(mut s) = v.live_slot().take() else { return };
    let mut got: Vec<Envelope> = Vec::new();
    let mut lost = false;
    match &s.link {
        Link::Local(l) => {
            while let Ok(w) = l.rx.try_recv() {
                match w {
                    cc_live::net::Wire::Bytes(b) => {
                        if let Ok(e) = Envelope::decode(&b) {
                            got.push(e);
                        }
                    }
                    cc_live::net::Wire::Close(_) => lost = true,
                }
            }
        }
        Link::Remote { rx, .. } => {
            while let Ok(r) = rx.try_recv() {
                match r {
                    Ok(e) => got.push(e),
                    Err(_) => lost = true,
                }
            }
        }
    }
    // the window's own steps first: they were made on the model as it was before what arrived now
    let steps = v.local_steps(&mut || s.next_group());
    let v0 = v.model_version();
    let c = s.doc.c;
    v.with_adapter(c, &mut |a| {
        for (label, ops, inverse) in &steps {
            s.doc.local_change(a, label, ops.clone(), inverse.clone());
        }
        for e in &got {
            s.doc.message(a, e);
        }
        s.doc.flush(a);
    });
    if lost {
        s.closed = true;
        s.doc.dropped();
    }
    let events = std::mem::take(&mut s.doc.events);
    let mut say: Option<String> = None;
    let someone = |v: &V, n: String| if n.is_empty() { v.tr("live.someone", &[]) } else { n };
    for ev in events {
        match ev {
            DocEvent::Code { code } => {
                s.code = code;
                // the new code's tag is what the LAN finds the session by
                if s.is_host() {
                    advertise(&mut s);
                }
            }
            DocEvent::Join { c, name, device, via, assertion, picture, aud } => {
                let (signed, refused, picture) = check_assertion(&assertion, &aud, &picture);
                s.knocks.retain(|k| k.c != c);
                s.knocks.push(Knock { c, name, device, via, signed, refused, picture });
            }
            DocEvent::Welcome { c, .. } => {
                v.claim_ids(c);
                say = Some(v.tr("live.status.live", &[]));
            }
            DocEvent::Status(st) if st == "waiting" => say = Some(v.tr("live.status.waiting", &[])),
            DocEvent::Status(st) if st == "reconnecting" => say = Some(v.tr("live.status.reconnecting", &[])),
            DocEvent::Refused { label, reason, by_name, .. } => {
                let key = match reason.as_str() {
                    "locked" => "live.refused.locked",
                    "role" => "live.refused.role",
                    _ => "live.refused",
                };
                let who = someone(v, by_name);
                say = Some(v.tr(key, &[("what", label), ("name", who)]));
            }
            DocEvent::UndoSkipped { label, by_name } => {
                let who = someone(v, by_name);
                say = Some(v.tr("live.undo_skipped", &[("what", label), ("name", who)]));
            }
            DocEvent::Ended { .. } => say = Some(v.tr("live.ended", &[])),
            _ => {}
        }
    }
    s.flush_out();
    let done = s.doc.final_();
    if !done {
        *v.live_slot() = Some(s);
    } else {
        drop(s);
        v.release_ids();
    }
    if let Some(m) = say {
        v.say(m);
    }
    if v.model_version() != v0 {
        v.after_remote();
    }
}

/// A step this window made: its ops go out (live.js's wrapped `change` / `step`). `ops` and `inverse` start with the
/// step's `{o:'g'}` mark (see [`LiveSession::next_group`]).
#[cfg(not(target_arch = "wasm32"))]
pub fn local_change<V: LiveView>(v: &mut V, label: &str, ops: Mp, inverse: Mp) {
    let Some(mut s) = v.live_slot().take() else { return };
    let c = s.doc.c;
    v.with_adapter(c, &mut |a| {
        s.doc.local_change(a, label, ops.clone(), inverse.clone());
    });
    s.flush_out();
    *v.live_slot() = Some(s);
}

/// Undo / redo in a live session: the person's own steps (live.js `client.undo()` / `redo()`), a whole group at a time
/// (cadcraft/dev/live.js `liveUndo`). → None when not live, Some(label) when something was undone.
#[cfg(not(target_arch = "wasm32"))]
pub fn reverse<V: LiveView>(v: &mut V, undo: bool) -> Option<Option<String>> {
    let mut s = v.live_slot().take()?;
    let label = if undo { s.doc.undo_label() } else { s.doc.redo_label() };
    let c = s.doc.c;
    let mut ok = false;
    v.with_adapter(c, &mut |a| {
        ok = if undo { s.doc.undo(a) } else { s.doc.redo(a) };
    });
    s.flush_out();
    *v.live_slot() = Some(s);
    v.after_remote();
    Some(if ok { label } else { None })
}

#[cfg(not(target_arch = "wasm32"))]
fn advertise(s: &mut LiveSession) {
    let Some((tag, _)) = cc_live::crypto::parse_code(&s.code) else { return };
    let port = s.addr.rsplit_once(':').and_then(|(_, p)| p.parse::<u16>().ok()).unwrap_or(0);
    if let Some(a) = s.advert.take() {
        a.stop();
    }
    // Only sessions open to the LAN are advertised (a test bound to 127.0.0.1 is not).
    let open = s.addr.starts_with("0.0.0.0") || s.lan.iter().any(|a| s.addr.starts_with(a.as_str()));
    if open && port > 0 && !s.lan.is_empty() {
        s.advert = cc_live::mdns::advertise(&tag, s.app, port, &s.lan).ok();
    }
}

/// The address a joiner was given: `host:port`, or a join link (`http://host:port/apps/<slug>/#live=<code>`), with the
/// code after it or in the link. → (host:port, code or "plain").
pub fn parse_join(text: &str) -> Option<(String, String)> {
    let t = text.trim();
    if let Some(rest) = t.strip_prefix("http://") {
        let (hostport, frag) = rest.split_once('/')?;
        let code = frag.split_once("#live=")?.1.trim();
        if hostport.is_empty() || hostport.len() > 80 || !hostport.contains(':') || code.is_empty() {
            return None;
        }
        return Some((hostport.to_string(), code.to_string()));
    }
    let (addr, code) = t.split_once(' ')?;
    Some((addr.trim().to_string(), code.trim().to_string()))
}

/// The `p.live.*` commands (see the module's doc).
#[cfg(not(target_arch = "wasm32"))]
pub fn run<V: LiveView>(v: &mut V, id: &str, name: &str) -> Result<(), String> {
    let what = id.strip_prefix(V::PREFIX).and_then(|x| x.strip_prefix(".live.")).ok_or_else(|| format!("unknown command {id}"))?;
    match what {
        "host" => {
            if v.live_slot().is_some() {
                return Err("live.host: this window is in a live session already".into());
            }
            let bind = if name.trim().is_empty() { "0.0.0.0:0" } else { name.trim() };
            let bind: std::net::SocketAddr = bind.parse().map_err(|_| format!("live.host: an address host:port, not {bind}"))?;
            let cfg = cc_live::server::Config { app: V::APP.into(), schema: 1, ..Default::default() };
            let h = cc_live::net::start(cfg, bind).map_err(|e| format!("live.host: {e}"))?;
            let local = h.attach_local();
            let addr = h.addr.to_string();
            let mut s = LiveSession {
                doc: LiveDoc::new(),
                link: Link::Local(local),
                server: Some(h),
                addr,
                code: String::new(),
                knocks: vec![],
                lan: cc_live::mdns::lan_addrs(),
                advert: None,
                app: V::APP,
                g: 0,
                closed: false,
            };
            let hello = s.doc.hello(V::APP, 1, "Host", Some(("host", "")));
            s.send(&hello);
            s.send(&Envelope::new("invite"));
            *v.live_slot() = Some(s);
            v.live_bind();
            pump(v, 2000, |s| !s.code.is_empty() && s.doc.is_live());
            if let Some(s) = v.live_slot().as_ref() {
                let line = hosting_line(s);
                v.say(line);
            }
        }
        "join" => {
            if v.live_slot().is_some() {
                return Err("live.join: this window is in a live session already".into());
            }
            let t = name.trim();
            let (addr, code) = match parse_join(t) {
                Some(x) => x,
                None => {
                    // a code alone: the session is found on the LAN by its tag (mDNS)
                    let (tag, _) = cc_live::crypto::parse_code(t).ok_or("live.join: <host:port> <code>, a join link, or a code")?;
                    let found = cc_live::mdns::browse(&tag, Duration::from_millis(1500))?;
                    let f = found
                        .into_iter()
                        .find(|f| f.app.is_empty() || f.app == V::APP)
                        .ok_or("live.join: no session with that code on this network; type its address")?;
                    (format!("{}:{}", f.addr, f.port), t.to_string())
                }
            };
            let secret = if code == "plain" { cc_live::net::Secret::Plain } else { cc_live::net::Secret::Code(code.clone()) };
            let mut j = cc_live::net::Joiner::connect(&addr, &secret)?;
            let doc = LiveDoc::new();
            let guest = v.tr("live.guest", &[]);
            j.send(&doc.hello(V::APP, 1, &guest, None))?;
            let tx = std::sync::Arc::new(std::sync::Mutex::new(j));
            let (etx, rx) = channel();
            let reader = tx.clone();
            std::thread::Builder::new()
                .name("cc-live-join".into())
                .spawn(move || {
                    loop {
                        let r = reader.lock().unwrap_or_else(std::sync::PoisonError::into_inner).recv(Duration::from_millis(20));
                        match r {
                            Ok(Some(e)) => {
                                if etx.send(Ok(e)).is_err() {
                                    break;
                                }
                            }
                            Ok(None) => std::thread::sleep(Duration::from_millis(2)),
                            Err(e) => {
                                let _ = etx.send(Err(e));
                                break;
                            }
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
            *v.live_slot() = Some(LiveSession {
                doc,
                link: Link::Remote { tx, rx },
                server: None,
                addr,
                code: String::new(),
                knocks: vec![],
                lan: vec![],
                advert: None,
                app: V::APP,
                g: 0,
                closed: false,
            });
            v.live_bind();
            pump(v, 10_000, |s| s.doc.is_live() || s.doc.final_() || s.doc.status == "waiting");
        }
        "approve" | "deny" => {
            let s = v.live_slot().as_mut().ok_or("live: not in a live session")?;
            let mut it = name.split_whitespace();
            let c: u64 = it.next().and_then(|x| x.parse().ok()).ok_or("live.approve: <client> [editor|viewer]")?;
            let b = if what == "approve" {
                let role = it.next().unwrap_or("editor");
                if role != "editor" && role != "viewer" {
                    return Err("live.approve: the role is editor or viewer".into());
                }
                Mp::map(vec![("c", Mp::from(c)), ("role", Mp::from(role))])
            } else {
                Mp::map(vec![("c", Mp::from(c))])
            };
            s.send(&Envelope::new(if what == "approve" { "approve" } else { "deny" }).body(b));
            s.knocks.retain(|k| k.c != c);
        }
        "window" => *v.live_window_open() = name.trim() != "close",
        "invite" => {
            let s = v.live_slot().as_mut().ok_or("live: not in a live session")?;
            s.send(&Envelope::new("invite"));
        }
        "leave" => {
            if let Some(mut s) = v.live_slot().take() {
                s.send(&Envelope::new(if s.server.is_some() { "end" } else { "bye" }));
                if let Link::Remote { tx, .. } = &s.link {
                    tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).close();
                }
                drop(s);
                v.release_ids();
            }
            let m = v.tr("live.ended", &[]);
            v.say(m);
        }
        "pump" => {
            let mut it = name.split_whitespace();
            let ms: u64 = it.next().and_then(|x| x.parse().ok()).unwrap_or(2000).min(120_000);
            let flags: Vec<&str> = it.collect();
            // `live`: until this window is in the session (let in, the snapshot loaded)
            let (wait, approve, live) = (flags.contains(&"wait"), flags.contains(&"approve"), flags.contains(&"live"));
            let t0 = web_time::Instant::now();
            loop {
                poll(v);
                let Some(s) = v.live_slot().as_mut() else { break };
                if approve {
                    for k in std::mem::take(&mut s.knocks) {
                        s.send(&Envelope::new("approve").body(Mp::map(vec![("c", Mp::from(k.c)), ("role", Mp::from("editor"))])));
                    }
                }
                let done = if live { s.doc.is_live() } else { !wait && s.doc.settled() && s.doc.outbox.is_empty() };
                if done || t0.elapsed() > Duration::from_millis(ms) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(3));
            }
            if let Some(s) = v.live_slot().as_ref() {
                let line = hosting_line(s);
                v.say(line);
            }
        }
        o => return Err(format!("unknown command {}.live.{o}", V::PREFIX)),
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
pub fn run<V: LiveView>(_v: &mut V, id: &str, _name: &str) -> Result<(), String> {
    Err(format!("{id}: Studio Live is not in the browser edition of this version yet"))
}

/// Carry messages until `done` (or `ms` passed): headless runs and the first moments of a session.
#[cfg(not(target_arch = "wasm32"))]
pub fn pump<V: LiveView>(v: &mut V, ms: u64, done: impl Fn(&LiveSession) -> bool) {
    let t0 = web_time::Instant::now();
    loop {
        poll(v);
        let Some(s) = v.live_slot().as_ref() else { return };
        if done(s) || t0.elapsed() > Duration::from_millis(ms) {
            return;
        }
        std::thread::sleep(Duration::from_millis(3));
    }
}

/// The command table rows of `p.live.*` for an app (id, label, shortcut, params doc).
pub fn command_rows(prefix: &'static str) -> Vec<(String, &'static str, &'static str, &'static str)> {
    [
        ("host", "Host a live session", "{name: \"[bind address]\"} (default the LAN on a free port)"),
        ("join", "Join a live session", "{name: \"<host:port> <code> | <join link> | <code>\"}"),
        ("approve", "Let a joiner in", "{name: \"<client> [editor|viewer]\"}"),
        ("deny", "Turn a joiner away", "{name: \"<client>\"}"),
        ("invite", "A new session code", "{}"),
        ("window", "Live window", "{name: open | close}"),
        ("leave", "Leave the live session", "{} (the host ends it)"),
        ("pump", "Carry live messages", "{name: \"<ms> [wait] [approve] [live]\"} headless runs"),
    ]
    .into_iter()
    .map(|(k, l, p)| (format!("{prefix}.live.{k}"), l, "", p))
    .collect()
}

/// The Live window (Studio's hosting and joining dialogs, in plain English: their words are Studio's, not in the 0.5
/// app catalogs): host on the LAN and show the code, the join QR and link, let joiners in (a tick for a signed-in name
/// that checked out), or join with a code (found on the LAN), an address and a code, or a join link; leave. Buttons push
/// command ids with their params into `acts`.
#[cfg(not(target_arch = "wasm32"))]
pub fn window<V: LiveView>(v: &mut V, ctx: &egui::Context, tk: &crate::theme::Tokens, acts: &mut Vec<(String, String)>) {
    use egui::RichText;
    let mut open = *v.live_window_open();
    if !open {
        return;
    }
    let p = V::PREFIX;
    let cmd = |k: &str| format!("{p}.live.{k}");
    let title = v.label_live_window();
    crate::shell::Dialog::new(RichText::new(title).strong())
        .modeless()
        .default_width(400.0)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .open(&mut open)
        .show(ctx, |ui| match v.live_slot().as_ref() {
            Some(s) => {
                ui.label(hosting_line(s));
                if s.server.is_some() {
                    if let Some(link) = s.join_link() {
                        ui.horizontal(|ui| {
                            if let Some(q) = crate::qr::encode(link.as_bytes()) {
                                crate::qr::paint(ui, &q, 132.0, tk);
                            }
                            ui.vertical(|ui| {
                                ui.label(RichText::new(format!("Code {}", s.code)).strong().size(16.0));
                                ui.label(RichText::new("Joiners type the code in their Join box (the network finds this session), scan the QR, or open the link:").color(tk.text_dim));
                                ui.label(RichText::new(&link).monospace().color(tk.text_dim));
                                ui.label(
                                    RichText::new(if s.advert.is_some() {
                                        "Others on this network find the session by its code."
                                    } else {
                                        "This network can't announce the session: joiners scan the QR or type the address."
                                    })
                                    .color(tk.text_dim),
                                );
                                if ui.add(crate::shell::Btn::new("New code")).clicked() {
                                    acts.push((cmd("invite"), String::new()));
                                }
                            });
                        });
                    }
                    for k in &s.knocks {
                        ui.horizontal(|ui| {
                            let who = match &k.signed {
                                Some(sg) => format!("{} ✔ (signed in{})", sg.name, if sg.verified { "" } else { ", email not verified" }),
                                None => k.name.clone(),
                            };
                            let dev = if k.device.is_empty() { String::new() } else { format!(" ({})", k.device) };
                            ui.label(format!("{who}{dev} wants to join"));
                            if let Some(r) = k.refused {
                                ui.label(RichText::new(format!("name not checked: {r}")).color(tk.text_dim));
                            }
                            if ui.add(crate::shell::Btn::new("Edit")).clicked() {
                                acts.push((cmd("approve"), format!("{} editor", k.c)));
                            }
                            if ui.add(crate::shell::Btn::new("View only")).clicked() {
                                acts.push((cmd("approve"), format!("{} viewer", k.c)));
                            }
                            if ui.add(crate::shell::Btn::new("Deny")).clicked() {
                                acts.push((cmd("deny"), k.c.to_string()));
                            }
                        });
                    }
                }
                for p in s.doc.peers.values().filter(|p| p.state != "gone" && p.state != "removed") {
                    ui.label(RichText::new(format!("{} ({})", p.name, p.role)).color(tk.text_dim));
                }
                if ui.add(crate::shell::Btn::new(if s.server.is_some() { "End the session" } else { "Leave" })).clicked() {
                    acts.push((cmd("leave"), String::new()));
                }
            }
            None => {
                ui.label(RichText::new("Work on this together with others on your network.").color(tk.text_dim));
                if ui.add(crate::shell::Btn::primary("Share live")).clicked() {
                    acts.push((cmd("host"), String::new()));
                }
                ui.separator();
                let fid = egui::Id::unique(("cc_live_join", p));
                let mut f: (String, String) = ctx.data(|d| d.get_temp(fid)).unwrap_or_default();
                ui.horizontal(|ui| {
                    let l = ui.label("Code");
                    ui.add(crate::shell::field(&mut f.1).desired_width(100.0).hint_text("K7-Q4M2X")).labelled_by(l.id);
                });
                ui.horizontal(|ui| {
                    let l = ui.label("Address (if the network doesn't find it)");
                    ui.add(crate::shell::field(&mut f.0).desired_width(180.0).hint_text("192.168.1.20:47620")).labelled_by(l.id);
                });
                let code = f.1.trim().to_string();
                let addr = f.0.trim().to_string();
                if ui.add_enabled(!code.is_empty() || addr.starts_with("http://"), crate::shell::Btn::new("Join")).clicked() {
                    let arg = if addr.starts_with("http://") { addr.clone() } else if addr.is_empty() { code.clone() } else { format!("{addr} {code}") };
                    acts.push((cmd("join"), arg));
                }
                ctx.data_mut(|d| d.insert_temp(fid, f));
            }
        });
    *v.live_window_open() = open;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn join_text_forms() {
        assert_eq!(parse_join("192.168.1.2:4000 K7-Q4M2X"), Some(("192.168.1.2:4000".into(), "K7-Q4M2X".into())));
        assert_eq!(
            parse_join("http://192.168.1.2:4000/apps/cadcraft/#live=K7-Q4M2X"),
            Some(("192.168.1.2:4000".into(), "K7-Q4M2X".into()))
        );
        assert_eq!(parse_join("http://x/apps/a/#live="), None);
        assert_eq!(parse_join("K7-Q4M2X"), None);
        let (s, r, p) = check_assertion("v1.nope", "x", &[]);
        assert!(s.is_none() && r == Some("format") && p.is_empty());
    }
}
