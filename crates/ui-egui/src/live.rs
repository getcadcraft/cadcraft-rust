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
    /// The app has made changes it has not handed to [`LiveView::local_steps`] yet (CadCraft: a command still running).
    fn holding_steps(&self) -> bool {
        false
    }
    /// Just before [`LiveView::local_steps`]: the session as it is now (CadCraft: the areas others hold).
    fn before_steps(&mut self, _doc: &cc_live::doc::LiveDoc) {}
    /// This window's presence (live.js `presence()`: cursor, view, selection, command), sent when it changes.
    fn presence(&mut self) -> Option<Mp> {
        None
    }
    /// The hash of the file open here while it is unchanged since it was opened ('' otherwise): the same-file join.
    fn file_hash(&mut self) -> String {
        String::new()
    }
    /// The app's words for a refused edit (its catalog's key).
    fn refused_key(&self, reason: &str) -> String {
        match reason {
            "locked" => "live.refused.locked",
            "role" => "live.refused.role",
            _ => "live.refused",
        }
        .into()
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
    /// `exporter("ticket")` of this window's first sealed connection: its way back in (a rejoin, a new host)
    pub ticket: Vec<u8>,
    /// the code on show: one-time, when it runs out
    pub code_one_time: bool,
    pub code_expires: Option<web_time::Instant>,
    /// what the code section says (a code rotated, carried, used up, stopped …)
    pub code_note: String,
    /// the "One-time code" box (the next New code)
    pub one_time: bool,
    /// codes this window stopped itself (New code, Stop inviting): their `code-gone` says nothing
    quiet: Vec<String>,
    /// the host's handover under way: (client, name)
    pub handing: Option<(u64, String)>,
    /// the old server after this window handed the session over: stopped a moment later, once `moved` reached everyone
    old_server: Option<(cc_live::net::LiveHandle, web_time::Instant)>,
    /// the window's message line (Studio's `st.msg`)
    pub msg: String,
    /// following the session to a new host: the addresses still to try, and how many tries so far
    retry: Option<Retry>,
    /// the presence last sent, and when
    presence_sent: (Option<web_time::Instant>, Vec<u8>),
    /// the host told the session which file it opened (`save {q: 0, hash}`)
    announced: bool,
    /// the selection the app last held (CadCraft: what is selected is held)
    pub sel_seen: Vec<u64>,
    /// the last messages this session said on the app's status line (headless checks)
    pub said: Vec<String>,
}

/// Rejoining (after a drop, or following `moved`): the addresses to try in turn, three rounds.
#[cfg(not(target_arch = "wasm32"))]
struct Retry {
    urls: Vec<String>,
    i: usize,
    tries: usize,
    at: web_time::Instant,
    /// the new host's name (moved), for the message when none answers
    host: String,
    /// connected, waiting for the welcome (since `at`)
    waiting: bool,
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
        if let Some((h, _)) = self.old_server.take() {
            h.stop();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl LiveSession {
    fn new(link: Link, waker: Waker, server: Option<cc_live::net::LiveHandle>, addr: String, app: &'static str, me: String) -> Self {
        LiveSession {
            doc: LiveDoc::new(),
            link,
            server,
            addr,
            code: String::new(),
            knocks: vec![],
            lan: vec![],
            advert: None,
            app,
            me,
            sent_at: None,
            waker,
            g: 0,
            closed: false,
            ticket: vec![],
            code_one_time: false,
            code_expires: None,
            code_note: String::new(),
            one_time: false,
            quiet: vec![],
            handing: None,
            old_server: None,
            msg: String::new(),
            retry: None,
            presence_sent: (None, vec![]),
            announced: false,
            sel_seen: vec![],
            said: vec![],
        }
    }
    /// The rejoin key: this window's ticket under `h:<c>`, context `rejoin:<session>` (LIVE-PROTOCOL § 8); plain when
    /// there is none (a test server's connection without a handshake).
    fn rejoin_secret(&self) -> cc_live::net::Secret {
        if self.ticket.len() == 32 && self.doc.c != 0 && !self.doc.session.is_empty() {
            cc_live::net::Secret::Key {
                kid: format!("h:{}", self.doc.c),
                secret: self.ticket.clone(),
                context: format!("rejoin:{}", self.doc.session),
            }
        } else {
            cc_live::net::Secret::Plain
        }
    }
    /// The name of client `c` in the window's words ("Someone" when unknown).
    pub fn who(&self, c: u64) -> String {
        let n = self.doc.name_of(c);
        if n.is_empty() { sw("window.live.noname", &[]) } else { n }
    }
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

/// The session as this window sees it (headless checks, `<p>.live.info`): who is in, holds, locks, others' presence,
/// the code, the window's message.
#[cfg(not(target_arch = "wasm32"))]
pub fn info(s: &LiveSession) -> serde_json::Value {
    use serde_json::json;
    let mut holds: Vec<(&String, &u64)> = s.doc.holds.iter().collect();
    holds.sort();
    let mut locks: Vec<&String> = s.doc.locks.iter().collect();
    locks.sort();
    let mut mine: Vec<&String> = s.doc.my_holds().iter().collect();
    mine.sort();
    let presence: serde_json::Map<String, serde_json::Value> =
        s.doc.presence.iter().map(|(c, p)| (c.to_string(), crate::livestate::to_json(p))).collect();
    let peers: Vec<serde_json::Value> =
        s.doc.peers.values().map(|p| json!({"c": p.c, "name": p.name, "role": p.role, "state": p.state})).collect();
    json!({
        "c": s.doc.c, "role": s.doc.role, "status": s.doc.status, "host": s.is_host(), "relocated": s.doc.relocated,
        "q": s.doc.q, "session": s.doc.session, "snapshots": s.doc.snapshots, "code": s.code, "one_time": s.code_one_time, "code_note": s.code_note,
        "msg": s.msg, "said": s.said,
        "holds": holds.into_iter().map(|(k, c)| (k.clone(), json!(c))).collect::<serde_json::Map<_, _>>(),
        "locks": locks, "my_holds": mine, "presence": presence, "peers": peers,
    })
}

/// The session's line, in 0.5 Studio's words: "Live, 3 people" (or "Viewing live"), and for the host the code and the
/// address ("The code: K7-Q4M2X · Address 192.168.1.20:4545").
#[cfg(not(target_arch = "wasm32"))]
pub fn hosting_line(s: &LiveSession) -> String {
    let n = people_in(s).to_string();
    let key = if s.doc.role == "viewer" { "window.live.status.viewing" } else { "window.live.status.live" };
    let status = sw(key, &[("n", n)]);
    if s.server.is_some() {
        let address = sw("window.live.code.address", &[("address", shown_addr(s))]);
        if s.code.is_empty() {
            // no code open (used, stopped, expired): nobody new can join on this network
            format!("{status} · {address}")
        } else {
            format!("{status} · {} · {address}", sw("window.live.code.label", &[("code", s.code.clone())]))
        }
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
    v.before_steps(&s.doc);
    let steps = v.local_steps(&mut || s.next_group());
    if !steps.is_empty() {
        s.sent_at = Some(web_time::Instant::now());
    }
    let v0 = v.model_version();
    let c = s.doc.c;
    let holding = v.holding_steps();
    v.with_adapter(c, &mut |a| {
        for (label, ops, inverse) in &steps {
            s.doc.local_change(a, label, ops.clone(), inverse.clone());
        }
        for e in &got {
            s.doc.message(a, e);
            // the session moved: nothing more from this connection counts (an `end` the old server sends after it)
            if e.t == "moved" && s.doc.relocated {
                break;
            }
        }
        // the app has changes of its own not sent yet (a command still running): others' commits wait in the inbox, so
        // they land after these are pending (rolled back and rebased over, as 0.5's live.js sends each step at once)
        if !holding {
            s.doc.flush(a);
        }
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
            DocEvent::Code { code, tag, one_time, carried, reason, expires } => {
                if s.quiet.contains(&tag) {
                    continue;
                }
                s.code = code;
                s.code_one_time = one_time;
                s.code_expires = Some(web_time::Instant::now() + Duration::from_millis(expires.min(86_400_000)));
                s.code_note = if reason == "wrong-tries" {
                    sw("window.live.code.rotated", &[])
                } else if carried {
                    sw("window.live.code.carried", &[])
                } else {
                    String::new()
                };
                // the new code's tag is what the LAN finds the session by
                if s.is_host() {
                    advertise(&mut s);
                }
            }
            DocEvent::CodeGone { kid, reason } => {
                if let Some(i) = s.quiet.iter().position(|q| *q == kid) {
                    if reason == "stopped" {
                        s.quiet.remove(i);
                    }
                    continue;
                }
                let mine = cc_live::crypto::parse_code(&s.code).map(|(t, _)| t);
                if !s.is_host() || (!kid.is_empty() && mine.as_deref() != Some(kid.as_str())) {
                    continue;
                }
                s.code.clear();
                s.code_expires = None;
                if let Some(a) = s.advert.take() {
                    a.stop();
                }
                let r = if matches!(reason.as_str(), "used" | "expired" | "stopped" | "wrong-tries") { reason.as_str() } else { "other" };
                s.code_note = sw(&format!("window.live.code.gone.{r}"), &[]);
            }
            DocEvent::Join { c, name, device, via, assertion, picture, aud } => {
                let (signed, refused, picture) = check_assertion(&assertion, &aud, &picture);
                s.knocks.retain(|k| k.c != c);
                s.knocks.push(Knock { c, name, device, via, signed, refused, picture });
            }
            DocEvent::Welcome { c, .. } => {
                v.claim_ids(c);
                // the host announces the file it opened, so joiners who have it skip the snapshot (LIVE-PROTOCOL § 7)
                if s.is_host() && !s.announced && !s.doc.relocated {
                    s.announced = true;
                    let hash = v.file_hash();
                    if !hash.is_empty() {
                        s.send(&Envelope::new("save").body(Mp::map(vec![("q", Mp::from(0u64)), ("hash", Mp::from(hash.as_str()))])));
                    }
                }
                if s.retry.take().is_none() {
                    say = Some(v.tr("live.status.live", &[]));
                }
            }
            DocEvent::Status(st) if st == "waiting" => say = Some(v.tr("live.status.waiting", &[])),
            DocEvent::Status(st) if st == "reconnecting" && s.retry.is_none() && s.handing.is_none() => {
                say = Some(v.tr("live.status.reconnecting", &[]));
            }
            DocEvent::Refused { label, reason, by, by_name, .. } => {
                let by_name = if by_name.is_empty() { by.map(|b| s.doc.name_of(b)).unwrap_or_default() } else { by_name };
                let who = someone(v, by_name);
                let key = v.refused_key(&reason);
                say = Some(v.tr(&key, &[("what", label), ("name", who)]));
            }
            DocEvent::UndoSkipped { label, by_name } => {
                let who = someone(v, by_name);
                say = Some(v.tr("live.undo_skipped", &[("what", label), ("name", who)]));
            }
            DocEvent::Ended { .. } => {
                let host = someone(v, s.doc.host_name());
                say = Some(v.tr("live.ended", &[("name", host)]));
            }
            DocEvent::HandoverOff { reason, .. } => {
                let name = s.handing.take().map_or_else(|| sw("window.live.noname", &[]), |h| h.1);
                let r = if matches!(reason.as_str(), "unknown" | "gone" | "start") { reason.as_str() } else { "other" };
                s.msg = sw(&format!("window.live.handover.off.{r}"), &[("name", name)]);
                say = Some(s.msg.clone());
            }
            DocEvent::Handover { from, state, .. } => {
                match take_over::<V>(&mut s, &state) {
                    Ok(()) => {
                        s.msg = sw("window.live.handover.youhost", &[("name", s.who(from))]);
                        say = Some(s.msg.clone());
                    }
                    Err(e) => {
                        // the old host is told (the connection goes, `handover-off gone`); this window comes back in
                        s.msg = e;
                        if let Link::Remote { tx, .. } = &s.link {
                            tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).close();
                        }
                        let addr = s.addr.clone();
                        s.retry = Some(Retry {
                            urls: vec![addr],
                            i: 0,
                            tries: 0,
                            at: web_time::Instant::now(),
                            host: String::new(),
                            waiting: false,
                        });
                        s.doc.dropped();
                    }
                }
            }
            DocEvent::Moved { host, addrs, port, ticket } => {
                if !ticket.is_empty() {
                    s.ticket = ticket;
                }
                let urls: Vec<String> = addrs
                    .iter()
                    // link-local IPv6 needs a zone id an address can't carry
                    .filter(|a| !a.to_ascii_lowercase().starts_with("fe8") && !a.to_ascii_lowercase().starts_with("fe9"))
                    .filter(|a| !a.to_ascii_lowercase().starts_with("fea") && !a.to_ascii_lowercase().starts_with("feb"))
                    .map(|a| if a.contains(':') { format!("[{a}]:{port}") } else { format!("{a}:{port}") })
                    .collect();
                let name = s.who(host);
                if s.is_host() {
                    // we handed it over: an editor from here on, in the new host's session
                    let to = s.handing.take().map_or_else(|| name.clone(), |h| h.1);
                    s.msg = sw("window.live.handover.moved", &[("name", to)]);
                    s.code.clear();
                    s.code_expires = None;
                    s.code_note.clear();
                    s.knocks.clear();
                    if let Some(a) = s.advert.take() {
                        a.stop();
                    }
                    if let Some(h) = s.server.take() {
                        s.old_server = Some((h, web_time::Instant::now()));
                    }
                } else {
                    s.msg = sw("window.live.moved.follow", &[("name", name.clone())]);
                }
                say = Some(s.msg.clone());
                if let Link::Remote { tx, rx } = &mut s.link {
                    tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).close();
                    // what the old connection still had queued (an `end` after `moved`) is dropped with it
                    *rx = channel().1;
                }
                s.retry = Some(Retry { urls, i: 0, tries: 0, at: web_time::Instant::now(), host: name, waiting: false });
            }
            _ => {}
        }
    }
    // a connection that dropped (not a move): back in with the rejoin ticket (LIVE-PROTOCOL § 8)
    if s.closed && s.retry.is_none() && !s.doc.final_() && !s.is_host() && s.doc.c != 0 {
        let addr = s.addr.clone();
        s.retry = Some(Retry { urls: vec![addr], i: 0, tries: 0, at: web_time::Instant::now(), host: String::new(), waiting: false });
    }
    if s.retry.is_some()
        && let Some(m) = rejoin::<V>(&mut s)
    {
        say = Some(m);
    }
    // the old server, once `moved` reached everyone on it
    if s.old_server.as_ref().is_some_and(|(_, t)| t.elapsed() > Duration::from_secs(3))
        && let Some((h, _)) = s.old_server.take()
    {
        h.stop();
    }
    // this window's presence (cursor, view, selection, command): at most every 33 ms, only when it changed
    if s.doc.is_live()
        && s.presence_sent.0.is_none_or(|t| t.elapsed() >= Duration::from_millis(33))
        && let Some(p) = v.presence()
        && let Ok(bytes) = cc_live::msgpack::pack(&p)
        && bytes != s.presence_sent.1
    {
        s.presence_sent = (Some(web_time::Instant::now()), bytes);
        s.doc.set_presence(p);
    }
    s.doc.tick(cc_clock::now_ms());
    s.flush_out();
    if let Some(m) = &say {
        if s.said.len() >= 50 {
            s.said.remove(0);
        }
        s.said.push(m.clone());
    }
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
            let waker: Waker = std::sync::Arc::default();
            let link = attach_local(&h, &waker)?;
            let addr = h.addr.to_string();
            let mut s = LiveSession::new(link, waker, Some(h), addr, V::APP, String::new());
            s.lan = cc_live::mdns::lan_addrs();
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
            let guest = if who.is_empty() { v.tr("live.guest", &[]) } else { who.chars().take(60).collect() };
            let mut doc = LiveDoc::new();
            // the same file as the host's (opened, unchanged since): only the edits since travel (LIVE-PROTOCOL § 7)
            doc.hash = v.file_hash();
            let hello = doc.hello(V::APP, 1, &guest, None);
            let waker: Waker = std::sync::Arc::default();
            let (link, ticket) = connect_remote(&addr, &secret, &hello, &waker)?;
            let mut s = LiveSession::new(link, waker, None, addr, V::APP, guest);
            s.doc = doc;
            s.ticket = ticket;
            *v.live_slot() = Some(s);
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
        "stop" | "invite" => {
            // Stop inviting: the code closes (people already in stay). New code: the one on show stops, a new one comes
            // (`invite onetime`: it works for one person).
            let s = v.live_slot().as_mut().ok_or("live: not in a live session")?;
            if !s.is_host() {
                return Err(format!("{id}: only the host invites"));
            }
            if let Some((tag, _)) = cc_live::crypto::parse_code(&s.code) {
                s.send(&Envelope::new("uninvite").body(Mp::map(vec![("kid", Mp::from(tag.as_str()))])));
                s.quiet.push(tag);
            }
            s.code.clear();
            s.code_expires = None;
            if let Some(a) = s.advert.take() {
                a.stop();
            }
            if what == "stop" {
                s.code_note = sw("window.live.code.stopped", &[]);
            } else {
                let one = match name.trim() {
                    "onetime" | "one-time" => true,
                    "" => s.one_time,
                    _ => false,
                };
                s.one_time = one;
                s.code_note.clear();
                s.send(&Envelope::new("invite").body(Mp::map(vec![("one_time", Mp::Bool(one))])));
                pump(v, 2000, |s| !s.code.is_empty());
                if let Some(s) = v.live_slot().as_ref() {
                    let line = hosting_line(s);
                    v.say(line);
                }
            }
        }
        "handover" => {
            // `<client | name>`: hand the session to an editor in it (LIVE-PROTOCOL § 4.7)
            let s = v.live_slot().as_mut().ok_or("live: not in a live session")?;
            if !s.is_host() {
                return Err(format!("{id}: only the host hands the session over"));
            }
            let t = name.trim();
            let c = t.parse::<u64>().ok().or_else(|| s.doc.peers.values().find(|p| p.name == t && p.c != s.doc.c).map(|p| p.c));
            let c = c.ok_or_else(|| format!("{id}: nobody called {t} is in the session"))?;
            let who = s.who(c);
            s.send(&Envelope::new("handover").body(Mp::map(vec![("to", Mp::from(c))])));
            s.msg = sw("window.live.handover.sending", &[("name", who.clone())]);
            s.handing = Some((c, who));
            let m = s.msg.clone();
            v.say(m);
        }
        "leave" => {
            let host = v.live_slot().as_ref().map_or_else(String::new, |s| if s.is_host() { s.me.clone() } else { s.doc.host_name() });
            let host = if host.is_empty() { v.tr("live.someone", &[]) } else { host };
            if let Some(mut s) = v.live_slot().take() {
                s.send(&Envelope::new(if s.server.is_some() { "end" } else { "bye" }));
                if let Link::Remote { tx, .. } = &s.link {
                    tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).close();
                }
                drop(s);
                v.release_ids();
            }
            let m = v.tr("live.ended", &[("name", host)]);
            v.say(m);
        }
        "pump" => {
            let mut it = name.split_whitespace();
            let ms: u64 = it.next().and_then(|x| x.parse().ok()).unwrap_or(2000).min(120_000);
            let flags: Vec<&str> = it.collect();
            // `live`: until this window is in the session (let in, the snapshot loaded)
            let (wait, approve, live) = (flags.contains(&"wait"), flags.contains(&"approve"), flags.contains(&"live"));
            // `moved`: until this window is in the session again after a host handoff
            let moved = flags.contains(&"moved");
            // `host`: until this window hosts the session (it was handed over)
            let hosting = flags.contains(&"host");
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
                let done = if hosting {
                    s.is_host() && s.doc.is_live() && s.retry.is_none()
                } else if moved {
                    s.doc.relocated && s.doc.is_live() && s.retry.is_none()
                } else if live {
                    s.doc.is_live()
                } else {
                    !wait && s.doc.settled() && s.doc.outbox.is_empty()
                };
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

/// This window is handed the session (LIVE-PROTOCOL § 4.7): a session server carrying on from the state, `moved`
/// with its addresses to everyone through the old host, and this window in on it with its resume token.
#[cfg(not(target_arch = "wasm32"))]
fn take_over<V: LiveView>(s: &mut LiveSession, state: &[u8]) -> Result<(), String> {
    let failed = || sw("window.live.handover.off.start", &[("name", sw("window.live.noname", &[]))]);
    let st = cc_live::msgpack::unpack(state).map_err(|_| failed())?;
    // a session reached on this machine stays on it (tests); else the LAN, on a free port
    let local = s.addr.starts_with("127.") || s.addr.starts_with("localhost") || s.addr.starts_with("[::1]");
    let bind: std::net::SocketAddr = if local { ([127, 0, 0, 1], 0).into() } else { ([0, 0, 0, 0], 0).into() };
    let cfg = cc_live::server::Config { app: V::APP.into(), schema: 1, ..Default::default() };
    let h = cc_live::net::start_from(cfg, bind, &st).map_err(|_| failed())?;
    let lan = cc_live::mdns::lan_addrs();
    let addrs: Vec<String> = if local || lan.is_empty() { vec!["127.0.0.1".into()] } else { lan.clone() };
    let moved = Mp::map(vec![
        ("addrs", Mp::Arr(addrs.iter().map(|a| Mp::from(a.as_str())).collect())),
        ("port", Mp::from(u64::from(h.addr.port()))),
    ]);
    s.send(&Envelope::new("moved").body(moved));
    if let Link::Remote { tx, .. } = &s.link {
        tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).close();
    }
    s.link = attach_local(&h, &s.waker)?;
    s.addr = h.addr.to_string();
    s.server = Some(h);
    s.lan = lan;
    s.closed = false;
    s.knocks.clear();
    s.doc.moved();
    let me = s.doc.c;
    s.doc.host_is(me);
    // no role claim after a move: the state made this window the host
    let hello = s.doc.hello(V::APP, 1, &s.me, None);
    s.send(&hello);
    Ok(())
}

/// Back into the session after a drop or a move: the next address, with the rejoin ticket, at most every 500 ms;
/// three rounds of every address, then the session is lost for this window (it keeps its copy).
#[cfg(not(target_arch = "wasm32"))]
fn rejoin<V: LiveView>(s: &mut LiveSession) -> Option<String> {
    let closed = s.closed;
    let r = s.retry.as_mut()?;
    if r.waiting && !closed {
        // in, waiting for the welcome: 3 s at most
        if r.at.elapsed() < Duration::from_secs(3) {
            return None;
        }
        if let Link::Remote { tx, .. } = &s.link {
            tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).close();
        }
    }
    r.waiting = false;
    if r.tries > 0 && r.at.elapsed() < Duration::from_millis(500) {
        return None;
    }
    if r.urls.is_empty() || r.tries >= r.urls.len().saturating_mul(3) {
        let host = std::mem::take(&mut r.host);
        s.retry = None;
        s.doc.lose();
        if host.is_empty() {
            return None;
        }
        s.msg = sw("window.live.end.moved", &[("host", host), ("file", sw("window.live.file.untitled", &[]))]);
        return Some(s.msg.clone());
    }
    let url = r.urls.get(r.i % r.urls.len()).cloned().unwrap_or_default();
    r.i = r.i.saturating_add(1);
    r.tries = r.tries.saturating_add(1);
    r.at = web_time::Instant::now();
    let secret = s.rejoin_secret();
    let hello = s.doc.hello(V::APP, 1, &s.me, None);
    if let Ok((link, _)) = connect_remote(&url, &secret, &hello, &s.waker) {
        s.link = link;
        s.addr = url;
        s.closed = false;
        if let Some(r) = s.retry.as_mut() {
            r.waiting = true;
            r.at = web_time::Instant::now();
        }
    }
    None
}

/// The host's own window on the server it runs (in-process): what the session sends it comes through a thread that
/// wakes the window.
#[cfg(not(target_arch = "wasm32"))]
fn attach_local(h: &cc_live::net::LiveHandle, waker: &Waker) -> Result<Link, String> {
    let mut local = h.attach_local();
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
    Ok(Link::Local(local))
}

/// A joiner's connection to `addr` (handshake for `secret`, then `hello`), its envelopes coming through a reader
/// thread that wakes the window. → the link and the connection's rejoin ticket (empty when plain).
#[cfg(not(target_arch = "wasm32"))]
fn connect_remote(addr: &str, secret: &cc_live::net::Secret, hello: &Envelope, waker: &Waker) -> Result<(Link, Vec<u8>), String> {
    let mut j = cc_live::net::Joiner::connect(addr, secret)?;
    j.send(hello)?;
    let ticket = j.ticket.clone();
    let tx = std::sync::Arc::new(std::sync::Mutex::new(j));
    let (etx, rx) = channel();
    let reader = tx.clone();
    let w2 = waker.clone();
    std::thread::Builder::new()
        .name("cc-live-join".into())
        .spawn(move || {
            loop {
                // a short wait: the window sends on the same connection and waits for this lock meanwhile
                let r = reader.lock().unwrap_or_else(std::sync::PoisonError::into_inner).recv(Duration::from_millis(1));
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
                        wake(&w2);
                        break;
                    }
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok((Link::Remote { tx, rx }, ticket))
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
        ("invite", "New code", "{name: \"[onetime]\"} (the code on show stops; onetime: it works for one person)"),
        ("stop", "Stop inviting", "{} (people already in stay)"),
        ("handover", "Hand over", "{name: \"<client | name>\"} (host: an editor in the session becomes the host)"),
        ("info", "Live session state", "{} headless checks: holds, locks, presence, people, saved as <app>.live-info.json"),
        ("remove", "Remove from the session", "{name: \"<client>\"}"),
        ("role", "Change a person's role", "{name: \"<client> editor|viewer\"}"),
        ("window", "Live window", "{name: open | close}"),
        ("leave", "Leave the live session", "{} (the host ends it)"),
        ("pump", "Carry live messages", "{name: \"<ms> [wait] [approve] [live] [moved] [host] [time]\"} headless runs"),
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
    let ho_id = egui::Id::unique(("cc_live_handover", p));
    let mut ask_ho: Option<u64> = ctx.data(|d| d.get_temp(ho_id));
    crate::shell::Dialog::new(RichText::new(sw("window.live.title", &[])).strong().family(crate::theme::bold_family()))
        .modeless()
        .default_width(420.0)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .open(&mut open)
        .show(ctx, |ui| match v.live_slot().as_ref() {
            Some(s) => {
                ui.label(hosting_line(s));
                if !s.msg.is_empty() {
                    ui.label(RichText::new(&s.msg).color(tk.accent));
                }
                let host = s.server.is_some();
                if host {
                    ui.add_space(4.0);
                    ui.label(RichText::new(sw("window.live.code.title", &[])).strong().family(crate::theme::bold_family()));
                    match s.join_link() {
                        Some(link) => {
                            ui.horizontal(|ui| {
                                if let Some(q) = crate::qr::encode(link.as_bytes()) {
                                    crate::qr::paint(ui, &q, 132.0, tk).on_hover_text(sw("window.live.qr.label", &[]));
                                }
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new(sw("window.live.code.label", &[("code", s.code.clone())]))
                                            .strong()
                                            .family(crate::theme::bold_family())
                                            .size(16.0),
                                    );
                                    if let Some(at) = s.code_expires {
                                        let left = at.saturating_duration_since(web_time::Instant::now()).as_secs();
                                        let line = if left == 0 {
                                            sw("window.live.code.expired", &[])
                                        } else {
                                            sw("window.live.code.expires", &[("time", format!("{}:{:02}", left / 60, left % 60))])
                                        };
                                        ui.label(RichText::new(line).color(tk.text_dim));
                                        ctx.request_repaint_after(std::time::Duration::from_millis(500));
                                    }
                                    if s.code_one_time {
                                        ui.label(RichText::new(sw("window.live.code.onetime.on", &[])).color(tk.text_dim));
                                    }
                                    ui.label(sw("window.live.code.address", &[("address", shown_addr(s))]));
                                    ui.label(RichText::new(sw("window.live.code.how", &[])).color(tk.text_dim));
                                    if !s.code_note.is_empty() {
                                        ui.label(RichText::new(&s.code_note).color(tk.accent));
                                    }
                                });
                            });
                        }
                        None => {
                            let note = if s.code_note.is_empty() { sw("window.live.code.none", &[]) } else { s.code_note.clone() };
                            ui.label(RichText::new(note).color(tk.text_dim));
                        }
                    }
                    // the one-time box and New code / Stop inviting (0.5 Studio's code section)
                    let oid = egui::Id::unique(("cc_live_onetime", p));
                    let mut one: bool = ctx.data(|d| d.get_temp(oid)).unwrap_or(s.one_time);
                    ui.checkbox(&mut one, sw("window.live.code.onetime", &[]));
                    ctx.data_mut(|d| d.insert_temp(oid, one));
                    ui.horizontal(|ui| {
                        if ui.add(crate::shell::Btn::new(sw("window.live.code.new", &[]))).clicked() {
                            acts.push((cmd("invite"), if one { "onetime".into() } else { "plain".into() }));
                        }
                        if !s.code.is_empty() && ui.add(crate::shell::Btn::new(sw("window.live.code.stop", &[]))).clicked() {
                            acts.push((cmd("stop"), String::new()));
                        }
                    });
                    if !s.knocks.is_empty() {
                        ui.add_space(4.0);
                        ui.label(RichText::new(sw("window.live.waiting.title", &[])).strong().family(crate::theme::bold_family()));
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
                ui.label(
                    RichText::new(sw("window.live.people.title", &[("n", people_in(s).to_string())]))
                        .strong()
                        .family(crate::theme::bold_family()),
                );
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
                            // hand the session to an editor in it (LIVE-PROTOCOL § 4.7)
                            if pr.role == "editor"
                                && ui
                                    .add_enabled(s.handing.is_none(), crate::shell::Btn::new(sw("window.live.handover", &[])))
                                    .on_hover_text(sw("window.live.handover.title", &[("name", name.clone())]))
                                    .clicked()
                            {
                                ask_ho = Some(pr.c);
                            }
                            if ui.add(crate::shell::Btn::new(sw("window.live.people.remove", &[]))).clicked() {
                                ask = Some(pr.c);
                            }
                        }
                    });
                    if ask_ho == Some(pr.c) {
                        ui.label(sw("window.live.handover.ask", &[("name", name.clone())]));
                        ui.horizontal(|ui| {
                            if ui.add(crate::shell::Btn::primary(sw("window.live.handover.yes", &[]))).clicked() {
                                acts.push((cmd("handover"), pr.c.to_string()));
                                ask_ho = None;
                            }
                            if ui.add(crate::shell::Btn::new(sw("window.live.cancel", &[]))).clicked() {
                                ask_ho = None;
                            }
                        });
                    }
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
                ui.label(RichText::new(sw("live.join.title", &[])).strong().family(crate::theme::bold_family()));
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
    ctx.data_mut(|d| match ask_ho {
        Some(c) => {
            d.insert_temp(ho_id, c);
        }
        None => d.remove::<u64>(ho_id),
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
            "window.live.code.stopped",
            "window.live.code.expires",
            "window.live.code.expired",
            "window.live.code.onetime",
            "window.live.code.onetime.on",
            "window.live.code.rotated",
            "window.live.code.carried",
            "window.live.code.gone.used",
            "window.live.code.gone.expired",
            "window.live.code.gone.stopped",
            "window.live.code.gone.wrong-tries",
            "window.live.code.gone.other",
            "window.live.handover",
            "window.live.handover.title",
            "window.live.handover.ask",
            "window.live.handover.yes",
            "window.live.handover.sending",
            "window.live.handover.moved",
            "window.live.handover.youhost",
            "window.live.handover.off.unknown",
            "window.live.handover.off.gone",
            "window.live.handover.off.start",
            "window.live.handover.off.other",
            "window.live.moved.follow",
            "window.live.end.moved",
            "window.live.file.untitled",
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
