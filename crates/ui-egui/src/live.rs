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

/// The window a session's threads wake when a message arrives.
pub type Waker = std::sync::Arc<std::sync::Mutex<Option<egui::Context>>>;

#[cfg(not(target_arch = "wasm32"))]
fn wake(w: &Waker) {
    if let Some(c) = w.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref() {
        c.request_repaint();
    }
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
    /// the name this window gave the session
    pub me: String,
    /// when this window last sent a step of its own (headless round-trip timings)
    pub sent_at: Option<web_time::Instant>,
    /// the window to wake when a message arrives (set each frame): others' edits show at once, not at the next repaint
    pub waker: Waker,
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
    /// The window to wake when a message arrives.
    pub fn set_waker(&self, ctx: &egui::Context) {
        let mut w = self.waker.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if w.is_none() {
            *w = Some(ctx.clone());
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

/// A word of 0.5 Studio's Live window and Join box (`window.live.*`, `live.join.*` in Studio's catalog, en + es-MX).
pub fn sw(key: &str, vars: &[(&str, String)]) -> String {
    crate::lang::t(&crate::lang::STUDIO, key, vars)
}

/// The people in the session, this window counted once.
#[cfg(not(target_arch = "wasm32"))]
pub fn people_in(s: &LiveSession) -> usize {
    let mut n = s.doc.peers.values().filter(|p| p.state != "gone" && p.state != "removed").count();
    if !s.doc.peers.contains_key(&s.doc.c) {
        n = n.saturating_add(1);
    }
    n
}

/// The address joiners type: this machine's on the LAN for a session open to it (not 0.0.0.0).
#[cfg(not(target_arch = "wasm32"))]
pub fn shown_addr(s: &LiveSession) -> String {
    match (s.addr.strip_prefix("0.0.0.0:"), s.lan.first()) {
        (Some(port), Some(ip)) => format!("{ip}:{port}"),
        _ => s.addr.clone(),
    }
}

/// The session's line, in 0.5 Studio's words: "Live, 3 people" (or "Viewing live"), and for the host the code and the
/// address ("The code: K7-Q4M2X · Address 192.168.1.20:4545").
#[cfg(not(target_arch = "wasm32"))]
pub fn hosting_line(s: &LiveSession) -> String {
    let n = people_in(s).to_string();
    let key = if s.doc.role == "viewer" { "window.live.status.viewing" } else { "window.live.status.live" };
    let status = sw(key, &[("n", n)]);
    if s.server.is_some() {
        let code = if s.code.is_empty() { "…".to_string() } else { s.code.clone() };
        format!(
            "{status} · {} · {}",
            sw("window.live.code.label", &[("code", code)]),
            sw("window.live.code.address", &[("address", shown_addr(s))])
        )
    } else {
        status
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
    if !steps.is_empty() {
        s.sent_at = Some(web_time::Instant::now());
    }
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
    s.sent_at = Some(web_time::Instant::now());
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
            // `[bind address] [| the name others see]`
            let (bind, who) = name.split_once('|').map_or((name.trim(), ""), |(a, b)| (a.trim(), b.trim()));
            let bind = if bind.is_empty() { "0.0.0.0:0" } else { bind };
            let bind: std::net::SocketAddr = bind.parse().map_err(|_| format!("live.host: an address host:port, not {bind}"))?;
            let cfg = cc_live::server::Config { app: V::APP.into(), schema: 1, ..Default::default() };
            let h = cc_live::net::start(cfg, bind).map_err(|e| format!("live.host: {e}"))?;
            let mut local = h.attach_local();
            // what the session sends the host's window comes through a thread that wakes the window
            let waker: Waker = std::sync::Arc::default();
            let (ftx, frx) = channel();
            let from = std::mem::replace(&mut local.rx, frx);
            let w2 = waker.clone();
            std::thread::Builder::new()
                .name("cc-live-host".into())
                .spawn(move || {
                    for m in from {
                        if ftx.send(m).is_err() {
                            break;
                        }
                        wake(&w2);
                    }
                })
                .map_err(|e| e.to_string())?;
            let addr = h.addr.to_string();
            let mut s = LiveSession {
                doc: LiveDoc::new(),
                link: Link::Local(local),
                waker,
                server: Some(h),
                addr,
                code: String::new(),
                knocks: vec![],
                lan: cc_live::mdns::lan_addrs(),
                advert: None,
                app: V::APP,
                me: String::new(),
                sent_at: None,
                g: 0,
                closed: false,
            };
            let me = if who.is_empty() { v.tr("live.guest", &[]) } else { who.chars().take(60).collect() };
            s.me = me.clone();
            let hello = s.doc.hello(V::APP, 1, &me, Some(("host", "")));
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
            // the name others see, after a "|" (0.5 Studio's join box asks for it)
            let (t, who) = name.split_once('|').map_or((name.trim(), ""), |(a, b)| (a.trim(), b.trim()));
            let (addr, code) = match parse_join(t) {
                Some(x) => x,
                None => {
                    // a code alone: the session is found on the LAN by its tag (mDNS)
                    let (tag, _) = cc_live::crypto::parse_code(t).ok_or_else(|| sw("live.join.bad.unknown", &[]))?;
                    let found = cc_live::mdns::browse(&tag, Duration::from_millis(1500))?;
                    let f = found.into_iter().find(|f| f.app.is_empty() || f.app == V::APP).ok_or_else(|| sw("live.join.notfound", &[]))?;
                    (format!("{}:{}", f.addr, f.port), t.to_string())
                }
            };
            let secret = if code == "plain" { cc_live::net::Secret::Plain } else { cc_live::net::Secret::Code(code.clone()) };
            let mut j = cc_live::net::Joiner::connect(&addr, &secret)?;
            let doc = LiveDoc::new();
            let guest = if who.is_empty() { v.tr("live.guest", &[]) } else { who.chars().take(60).collect() };
            j.send(&doc.hello(V::APP, 1, &guest, None))?;
            let tx = std::sync::Arc::new(std::sync::Mutex::new(j));
            let (etx, rx) = channel();
            let reader = tx.clone();
            let waker: Waker = std::sync::Arc::default();
            let w2 = waker.clone();
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
                                wake(&w2);
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
                me: guest.clone(),
                doc,
                link: Link::Remote { tx, rx },
                waker,
                server: None,
                addr,
                code: String::new(),
                knocks: vec![],
                lan: vec![],
                advert: None,
                app: V::APP,
                sent_at: None,
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
        "remove" | "role" => {
            let s = v.live_slot().as_mut().ok_or("live: not in a live session")?;
            let mut it = name.split_whitespace();
            let c: u64 = it.next().and_then(|x| x.parse().ok()).ok_or("live.remove: <client>")?;
            let b = if what == "role" {
                let role = it.next().unwrap_or("editor");
                if role != "editor" && role != "viewer" {
                    return Err("live.role: the role is editor or viewer".into());
                }
                Mp::map(vec![("c", Mp::from(c)), ("role", Mp::from(role))])
            } else {
                Mp::map(vec![("c", Mp::from(c))])
            };
            s.send(&Envelope::new(what).body(b));
        }
        "stop" => {
            // Stop inviting: the code closes (people already in stay)
            let s = v.live_slot().as_mut().ok_or("live: not in a live session")?;
            s.send(&Envelope::new("uninvite"));
            s.code.clear();
            if let Some(a) = s.advert.take() {
                a.stop();
            }
        }
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
            // `time`: the status line says how long it took (headless timings)
            let time = flags.contains(&"time");
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
                std::thread::sleep(if time { Duration::from_micros(100) } else { Duration::from_millis(3) });
            }
            if let Some(s) = v.live_slot().as_ref() {
                let mut line = hosting_line(s);
                if time {
                    // from this window's last step sent (else from the pump's start) to everything it sent decided
                    let from = s.sent_at.unwrap_or(t0);
                    line.push_str(&format!(" · settled in {:.2} ms", from.elapsed().as_secs_f64() * 1000.0));
                }
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
        ("join", "Join a live session", "{name: \"<host:port> <code> | <join link> | <code> [| your name]\"}"),
        ("approve", "Let a joiner in", "{name: \"<client> [editor|viewer]\"}"),
        ("deny", "Turn a joiner away", "{name: \"<client>\"}"),
        ("invite", "New code", "{}"),
        ("stop", "Stop inviting", "{} (people already in stay)"),
        ("remove", "Remove from the session", "{name: \"<client>\"}"),
        ("role", "Change a person's role", "{name: \"<client> editor|viewer\"}"),
        ("window", "Live window", "{name: open | close}"),
        ("leave", "Leave the live session", "{} (the host ends it)"),
        ("pump", "Carry live messages", "{name: \"<ms> [wait] [approve] [live] [time]\"} headless runs"),
    ]
    .into_iter()
    .map(|(k, l, p)| (format!("{prefix}.live.{k}"), l, "", p))
    .collect()
}

/// The join field (0.5 Studio's "Join" box: the code alone, found on this network, or the address shown under the
/// host's code followed by the code, or a join link) → `<p>.live.join`'s argument, or the Studio key saying what is wrong.
pub fn join_words(text: &str) -> Result<String, &'static str> {
    let t = text.trim();
    if t.is_empty() {
        return Err("live.join.bad.empty");
    }
    if t.starts_with("http://") {
        return parse_join(t).map(|(a, c)| format!("{a} {c}")).ok_or("live.join.bad.address");
    }
    let mut it = t.split_whitespace();
    let (Some(a), b, None) = (it.next(), it.next(), it.next()) else { return Err("live.join.bad.unknown") };
    let is_addr = |x: &str| x.rsplit_once(':').is_some_and(|(h, p)| !h.is_empty() && p.parse::<u16>().is_ok());
    let is_code = |x: &str| cc_live::crypto::parse_code(x).is_some();
    match b {
        Some(code) if is_addr(a) && is_code(code) => Ok(format!("{a} {code}")),
        Some(_) if is_addr(a) => Err("live.join.bad.code"),
        None if is_addr(a) => Err("live.join.needcode"),
        None if is_code(a) => Ok(a.to_string()),
        None if a.contains(':') => Err("live.join.bad.address"),
        None => Err("live.join.bad.code"),
        Some(_) => Err("live.join.bad.unknown"),
    }
}

/// The Live window, as 0.5 Studio's (`window.live.*` in Studio's catalog): Share live; the code with its picture to
/// scan and the address, New code, Stop inviting; the knocks (a tick for a signed-in name that checked out) with Edit /
/// View only / Deny; the people in the session with their roles (the host changes a role or removes someone); End the
/// session / Leave; with no session, the Join box (the code, or the address and the code, and the name others see).
/// Buttons push command ids with their params into `acts`.
#[cfg(not(target_arch = "wasm32"))]
pub fn window<V: LiveView>(v: &mut V, ctx: &egui::Context, tk: &crate::theme::Tokens, acts: &mut Vec<(String, String)>) {
    use egui::RichText;
    let mut open = *v.live_window_open();
    if !open {
        return;
    }
    let p = V::PREFIX;
    let cmd = |k: &str| format!("{p}.live.{k}");
    let ask_id = egui::Id::unique(("cc_live_remove", p));
    let mut ask: Option<u64> = ctx.data(|d| d.get_temp(ask_id));
    crate::shell::Dialog::new(RichText::new(sw("window.live.title", &[])).strong())
        .modeless()
        .default_width(420.0)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .open(&mut open)
        .show(ctx, |ui| match v.live_slot().as_ref() {
            Some(s) => {
                ui.label(hosting_line(s));
                let host = s.server.is_some();
                if host {
                    ui.add_space(4.0);
                    ui.label(RichText::new(sw("window.live.code.title", &[])).strong());
                    match s.join_link() {
                        Some(link) => {
                            ui.horizontal(|ui| {
                                if let Some(q) = crate::qr::encode(link.as_bytes()) {
                                    crate::qr::paint(ui, &q, 132.0, tk).on_hover_text(sw("window.live.qr.label", &[]));
                                }
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(sw("window.live.code.label", &[("code", s.code.clone())])).strong().size(16.0));
                                    ui.label(sw("window.live.code.address", &[("address", shown_addr(s))]));
                                    ui.label(RichText::new(sw("window.live.code.how", &[])).color(tk.text_dim));
                                    ui.horizontal(|ui| {
                                        if ui.add(crate::shell::Btn::new(sw("window.live.code.new", &[]))).clicked() {
                                            acts.push((cmd("invite"), String::new()));
                                        }
                                        if ui.add(crate::shell::Btn::new(sw("window.live.code.stop", &[]))).clicked() {
                                            acts.push((cmd("stop"), String::new()));
                                        }
                                    });
                                });
                            });
                        }
                        None => {
                            ui.label(RichText::new(sw("window.live.code.none", &[])).color(tk.text_dim));
                            if ui.add(crate::shell::Btn::new(sw("window.live.code.new", &[]))).clicked() {
                                acts.push((cmd("invite"), String::new()));
                            }
                        }
                    }
                    if !s.knocks.is_empty() {
                        ui.add_space(4.0);
                        ui.label(RichText::new(sw("window.live.waiting.title", &[])).strong());
                    }
                    for k in &s.knocks {
                        let name = match &k.signed {
                            Some(sg) => format!("{} ✔", sg.name),
                            None if k.name.is_empty() => sw("window.live.noname", &[]),
                            None => k.name.clone(),
                        };
                        let device = if k.device.is_empty() { sw("window.live.nodevice", &[]) } else { k.device.clone() };
                        let via = sw(if k.via == "relay" { "window.live.via.relay.short" } else { "window.live.via.lan.short" }, &[]);
                        ui.label(format!("{} · {via}", sw("window.live.knock.card", &[("name", name), ("device", device)])));
                        ui.horizontal(|ui| {
                            if ui.add(crate::shell::Btn::primary(sw("window.live.approve.edit", &[]))).clicked() {
                                acts.push((cmd("approve"), format!("{} editor", k.c)));
                            }
                            if ui.add(crate::shell::Btn::new(sw("window.live.approve.view", &[]))).clicked() {
                                acts.push((cmd("approve"), format!("{} viewer", k.c)));
                            }
                            if ui.add(crate::shell::Btn::new(sw("window.live.approve.deny", &[]))).clicked() {
                                acts.push((cmd("deny"), k.c.to_string()));
                            }
                        });
                    }
                }
                ui.add_space(4.0);
                ui.label(RichText::new(sw("window.live.people.title", &[("n", people_in(s).to_string())])).strong());
                if !s.doc.peers.contains_key(&s.doc.c) {
                    let role = match s.doc.role.as_str() {
                        "host" => sw("window.live.role.host", &[]),
                        "viewer" => sw("window.live.role.viewer", &[]),
                        _ => sw("window.live.role.editor", &[]),
                    };
                    let name = if s.me.is_empty() { sw("window.live.noname", &[]) } else { s.me.clone() };
                    ui.label(RichText::new(format!("{name} {} · {role}", sw("window.live.people.you", &[]))).color(tk.text_dim));
                }
                for pr in s.doc.peers.values().filter(|p| p.state != "gone" && p.state != "removed") {
                    let role = match pr.role.as_str() {
                        "host" => sw("window.live.role.host", &[]),
                        "viewer" => sw("window.live.role.viewer", &[]),
                        "assistant" => sw("window.live.role.assistant", &[]),
                        _ => sw("window.live.role.editor", &[]),
                    };
                    let name = if pr.name.is_empty() { sw("window.live.noname", &[]) } else { pr.name.clone() };
                    let you = if pr.c == s.doc.c { format!(" {}", sw("window.live.people.you", &[])) } else { String::new() };
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(format!("{name}{you} · {role}")).color(tk.text_dim));
                        if host && pr.c != s.doc.c && matches!(pr.role.as_str(), "editor" | "viewer") {
                            let (to, key) = if pr.role == "viewer" {
                                ("editor", "window.live.role.editor")
                            } else {
                                ("viewer", "window.live.role.viewer")
                            };
                            if ui
                                .add(crate::shell::Btn::new(sw(key, &[])))
                                .on_hover_text(sw("window.live.people.role", &[("name", name.clone())]))
                                .clicked()
                            {
                                acts.push((cmd("role"), format!("{} {to}", pr.c)));
                            }
                            if ui.add(crate::shell::Btn::new(sw("window.live.people.remove", &[]))).clicked() {
                                ask = Some(pr.c);
                            }
                        }
                    });
                    if ask == Some(pr.c) {
                        ui.label(sw("window.live.people.remove.ask", &[("name", name.clone())]));
                        ui.horizontal(|ui| {
                            if ui.add(crate::shell::Btn::primary(sw("window.live.people.remove", &[]))).clicked() {
                                acts.push((cmd("remove"), pr.c.to_string()));
                                ask = None;
                            }
                            if ui.add(crate::shell::Btn::new(sw("window.live.cancel", &[]))).clicked() {
                                ask = None;
                            }
                        });
                    }
                }
                ui.add_space(4.0);
                let (end, hint) =
                    if host { ("window.live.end", "window.live.end.hint") } else { ("window.live.leave", "window.live.leave.hint") };
                if ui.add(crate::shell::Btn::new(sw(end, &[]))).on_hover_text(sw(hint, &[])).clicked() {
                    acts.push((cmd("leave"), String::new()));
                }
            }
            None => {
                ui.label(RichText::new(sw("window.live.share.pitch", &[])).color(tk.text_dim));
                ui.label(RichText::new(sw("window.live.share.free", &[])).color(tk.text_dim));
                // the name others see (0.5: "type the name others should see, then choose Share live"), for both
                let fid = egui::Id::unique(("cc_live_join", p));
                let mut f: (String, String, String) = ctx.data(|d| d.get_temp(fid)).unwrap_or_default();
                let l = ui.label(sw("window.live.name", &[]));
                ui.add(crate::shell::field(&mut f.2).desired_width(200.0)).labelled_by(l.id);
                ui.label(RichText::new(sw("window.live.name.hint", &[])).color(tk.text_dim));
                if ui.add(crate::shell::Btn::primary(sw("window.live.share", &[]))).clicked() {
                    let who = f.2.trim();
                    acts.push((cmd("host"), if who.is_empty() { String::new() } else { format!("| {who}") }));
                }
                ui.separator();
                ui.label(RichText::new(sw("live.join.title", &[])).strong());
                ui.label(RichText::new(sw("live.join.hint", &[])).color(tk.text_dim));
                let l = ui.label(sw("live.join.label", &[]));
                ui.add(crate::shell::field(&mut f.0).desired_width(260.0).hint_text("K7-Q4M2X")).labelled_by(l.id);
                if !f.1.is_empty() {
                    ui.label(RichText::new(&f.1).color(tk.accent));
                }
                if ui.add_enabled(!f.0.trim().is_empty(), crate::shell::Btn::new(sw("live.join.go", &[]))).clicked() {
                    match join_words(&f.0) {
                        Ok(spec) => {
                            f.1.clear();
                            let who = f.2.trim();
                            acts.push((cmd("join"), if who.is_empty() { spec } else { format!("{spec} | {who}") }));
                        }
                        Err(key) => f.1 = sw(key, &[]),
                    }
                }
                ctx.data_mut(|d| d.insert_temp(fid, f));
            }
        });
    ctx.data_mut(|d| match ask {
        Some(c) => {
            d.insert_temp(ask_id, c);
        }
        None => d.remove::<u64>(ask_id),
    });
    *v.live_window_open() = open;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_word_of_the_window_is_in_studios_catalog() {
        for k in [
            "window.live.title",
            "window.live.share",
            "window.live.share.pitch",
            "window.live.share.free",
            "window.live.code.title",
            "window.live.code.label",
            "window.live.code.address",
            "window.live.code.how",
            "window.live.code.none",
            "window.live.code.new",
            "window.live.code.stop",
            "window.live.qr.label",
            "window.live.knock.card",
            "window.live.nodevice",
            "window.live.via.lan.short",
            "window.live.via.relay.short",
            "window.live.approve.edit",
            "window.live.approve.view",
            "window.live.approve.deny",
            "window.live.waiting.title",
            "window.live.people.title",
            "window.live.people.you",
            "window.live.people.role",
            "window.live.people.remove",
            "window.live.people.remove.ask",
            "window.live.cancel",
            "window.live.role.host",
            "window.live.role.editor",
            "window.live.role.viewer",
            "window.live.role.assistant",
            "window.live.noname",
            "window.live.end",
            "window.live.end.hint",
            "window.live.leave",
            "window.live.leave.hint",
            "window.live.status.live",
            "window.live.status.viewing",
            "window.live.button",
            "window.live.button.title",
            "live.join.title",
            "live.join.hint",
            "live.join.label",
            "live.join.yourname",
            "live.join.name.guest",
            "live.join.go",
            "live.join.notfound",
            "live.join.needcode",
            "live.join.bad.empty",
            "live.join.bad.code",
            "live.join.bad.address",
            "live.join.bad.unknown",
        ] {
            for l in [cc_lang::Locale::En, cc_lang::Locale::EsMx] {
                assert!(crate::lang::STUDIO.get_in(l, k).is_some(), "{k} {l:?}");
            }
        }
        assert_eq!(join_words(" 192.168.1.20:4545  K7-Q4M2X "), Ok("192.168.1.20:4545 K7-Q4M2X".into()));
        assert_eq!(join_words("k7-q4m2x"), Ok("k7-q4m2x".into()));
        assert_eq!(join_words("192.168.1.20:4545"), Err("live.join.needcode"));
        assert_eq!(join_words("192.168.1.20:4545 nope"), Err("live.join.bad.code"));
        assert_eq!(join_words(""), Err("live.join.bad.empty"));
        assert_eq!(join_words("a b c"), Err("live.join.bad.unknown"));
        assert_eq!(join_words("http://10.0.0.2:4000/apps/cadcraft/#live=K7-Q4M2X"), Ok("10.0.0.2:4000 K7-Q4M2X".into()));
    }

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
