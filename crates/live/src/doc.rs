//! A Live client's document side (live-client.js `LiveClient` without its socket; LIVE-PROTOCOL.md § 4–8): the app's
//! steps go out as edits at once (applied already, kept pending), commits come in and are applied in order with the
//! pending edits rolled back and rebased over them, refused edits are taken back, undo / redo are per person (an undo of
//! a property someone wrote over since is skipped, and says who), snapshots are loaded (and made, for the host's window),
//! key versions are learned from commits and `revs`. The app's side is an [`Adapter`] (spec § 4); the transport is the
//! caller's: [`LiveDoc::message`] takes what arrives, [`LiveDoc::outbox`] holds what to send.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::envelope::{Envelope, PROTOCOL};
use crate::msgpack::Value;

/// A `touched` entry's version condition as the adapter gives it (live-client.js: `r` undefined / null / a number).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cond {
    /// undefined: a structural write is conditioned on the key's version as this client knows it
    Auto,
    /// null: no condition
    None,
    At(u64),
}

/// One `touched` entry from the adapter.
#[derive(Debug, Clone, PartialEq)]
pub struct TouchReq {
    pub k: String,
    pub m: char,
    pub f: Option<String>,
    pub r: Cond,
    pub g: bool,
}

/// The app's side (spec § 4): ops are the app's own MessagePack values, opaque to the server.
pub trait Adapter {
    /// Apply ops to the model (skipping ops on objects that don't exist). `origin`: own, remote, rollback, rebase, undo, redo.
    fn apply(&mut self, ops: &Value, origin: &str) -> Result<(), String>;
    /// The ops that undo `ops` on the model as it is now (called just before applying them).
    fn invert(&mut self, ops: &Value) -> Result<Value, String>;
    fn touched(&self, ops: &Value) -> Vec<TouchReq>;
    /// The committed model's bytes (the host's window answers a joiner's snapshot request).
    fn snapshot(&mut self) -> Vec<u8>;
    fn load(&mut self, bytes: &[u8], q: u64) -> Result<(), String>;
    /// The replica's check of an edit (None: fine).
    fn validate(&mut self, _ops: &Value) -> Option<String> {
        None
    }
}

/// What happened, for the app's status line and panels.
#[derive(Debug, Clone, PartialEq)]
pub enum DocEvent {
    Status(String),
    Welcome { c: u64, role: String },
    Refused { label: String, reason: String, by: Option<u64>, by_name: String, undo: bool },
    UndoSkipped { label: String, by_name: String },
    Join { c: u64, name: String },
    Code { code: String },
    Peer { c: u64 },
    Applied { commits: usize },
    Saved { q: u64 },
    Ended { reason: String },
}

#[derive(Debug, Clone)]
struct Pending {
    n: u64,
    label: String,
    ops: Value,
    inverse: Value,
    touched: Vec<TouchReq>,
    applied: bool,
    kind: &'static str,
    /// the undo / redo entry this reverses
    of: Option<Entry>,
}

#[derive(Debug, Clone)]
struct Entry {
    label: String,
    inverse: Value,
    touched: Vec<TouchReq>,
    q: u64,
}

#[derive(Debug, Clone)]
enum In {
    Commit { q: u64, c: u64, n: u64, label: String, ops: Value, touched: Vec<TouchReq> },
    Refuse { n: u64, reason: String, by: Option<u64>, by_name: String },
    End { reason: String },
}

/// A snapshot being received: its chunks, how many, its seq and encoding.
type Snap = (Vec<Option<Vec<u8>>>, usize, u64, String);

/// A peer as this client knows it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Peer {
    pub c: u64,
    pub name: String,
    pub role: String,
    pub state: String,
}

/// The document side of a Live client.
#[derive(Debug, Default)]
pub struct LiveDoc {
    pub status: String,
    pub c: u64,
    pub role: String,
    pub prefix: String,
    pub token: String,
    pub session: String,
    /// the last applied seq
    pub q: u64,
    n: u64,
    pending: Vec<Pending>,
    inbox: Vec<In>,
    undo_stack: Vec<Entry>,
    redo_stack: Vec<Entry>,
    revs: HashMap<String, u64>,
    prop_revs: HashMap<(String, String), (u64, u64)>,
    pub peers: BTreeMap<u64, Peer>,
    pub holds: HashMap<String, u64>,
    pub locks: HashSet<String>,
    my_holds: HashSet<String>,
    pub saved: (u64, String),
    welcomed: bool,
    ever_welcomed: bool,
    snap: Option<Snap>,
    loading: bool,
    /// envelopes to send, in order
    pub outbox: Vec<Envelope>,
    pub events: Vec<DocEvent>,
}

fn s_of(v: Option<&Value>) -> String {
    v.and_then(Value::as_str).unwrap_or("").to_string()
}

fn touches_of(v: Option<&Value>) -> Vec<TouchReq> {
    v.map(Value::as_arr)
        .unwrap_or(&[])
        .iter()
        .take(100_000)
        .filter_map(|t| {
            Some(TouchReq {
                k: t.get("k")?.as_str()?.to_string(),
                m: t.get("m").and_then(Value::as_str).and_then(|m| m.chars().next()).unwrap_or('s'),
                f: t.get("f").and_then(Value::as_str).map(str::to_string),
                r: t.get("r").and_then(Value::as_u64).map_or(Cond::Auto, Cond::At),
                g: false,
            })
        })
        .collect()
}

fn touched_value(ts: &[TouchReq]) -> Value {
    Value::Arr(
        ts.iter()
            .map(|t| {
                let mut m = vec![("k".to_string(), Value::Str(t.k.clone())), ("m".to_string(), Value::Str(t.m.to_string()))];
                if let Some(f) = &t.f {
                    m.push(("f".into(), Value::Str(f.clone())));
                }
                if let Cond::At(r) = t.r {
                    m.push(("r".into(), Value::from(r)));
                }
                if t.g {
                    m.push(("g".into(), Value::from(1u64)));
                }
                Value::Map(m)
            })
            .collect(),
    )
}

/// zlib inflate (CompressionStream 'deflate'), at most 512 MB out.
fn inflate(b: &[u8]) -> Option<Vec<u8>> {
    miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(b, 512 << 20).ok()
}

impl LiveDoc {
    pub fn new() -> Self {
        LiveDoc { status: "idle".into(), ..Default::default() }
    }

    fn set_status(&mut self, s: &str) {
        if self.status != s {
            self.status = s.to_string();
            self.events.push(DocEvent::Status(s.to_string()));
        }
    }

    pub fn is_live(&self) -> bool {
        self.status == "live"
    }
    pub fn final_(&self) -> bool {
        matches!(self.status.as_str(), "ended" | "denied" | "removed" | "lost" | "left" | "failed")
    }

    /// `hello` (a host's window adds role and token; a rejoin carries `resume`).
    pub fn hello(&self, app: &str, schema: u64, name: &str, role: Option<(&str, &str)>) -> Envelope {
        let mut b =
            vec![("v", Value::from(PROTOCOL)), ("app", Value::from(app)), ("schema", Value::from(schema)), ("name", Value::from(name))];
        if let Some((r, tok)) = role {
            b.push(("role", Value::from(r)));
            if !tok.is_empty() {
                b.push(("token", Value::from(tok)));
            }
        }
        if self.ever_welcomed && self.c != 0 {
            let holds: Vec<Value> = self.my_holds.iter().map(|k| Value::Str(k.clone())).collect();
            b.push((
                "resume",
                Value::map(vec![
                    ("c", Value::from(self.c)),
                    ("token", Value::from(self.token.as_str())),
                    ("q", Value::from(self.q)),
                    ("n", Value::from(self.pending.first().map_or(self.n.saturating_add(1), |p| p.n))),
                    ("holds", Value::Arr(holds)),
                ]),
            ));
        }
        Envelope::new("hello").body(Value::map(b))
    }

    /// The connection dropped: nothing more goes out until the next `welcome`.
    pub fn dropped(&mut self) {
        self.welcomed = false;
        if !self.final_() {
            self.set_status("reconnecting");
        }
    }

    /// One message from the session.
    pub fn message(&mut self, a: &mut dyn Adapter, m: &Envelope) {
        let b = &m.b;
        match m.t.as_str() {
            "waiting" => self.set_status("waiting"),
            "welcome" => self.welcome(m),
            "deny" => {
                let st = if self.ever_welcomed && s_of(b.get("reason")) == "resume" { "lost" } else { "denied" };
                self.set_status(st);
            }
            "removed" => self.set_status("removed"),
            "end" => self.inbox.push(In::End { reason: s_of(b.get("reason")) }),
            "commit" => self.inbox.push(In::Commit {
                q: m.q,
                c: b.get("c").and_then(Value::as_u64).unwrap_or(m.c),
                n: b.get("n").and_then(Value::as_u64).unwrap_or(m.n),
                label: s_of(b.get("label")),
                ops: b.get("ops").cloned().unwrap_or(Value::Nil),
                touched: touches_of(b.get("touched")),
            }),
            "refuse" => self.inbox.push(In::Refuse {
                n: b.get("n").and_then(Value::as_u64).unwrap_or(m.n),
                reason: s_of(b.get("reason")),
                by: b.get("by").and_then(Value::as_u64),
                by_name: s_of(b.get("by_name")),
            }),
            "snap" => self.snap_chunk(a, m),
            "revs" => {
                if let Some(Value::Map(keys)) = b.get("keys") {
                    for (k, r) in keys {
                        if let Some(r) = r.as_u64()
                            && r > self.revs.get(k).copied().unwrap_or(0)
                        {
                            self.revs.insert(k.clone(), r);
                        }
                    }
                }
            }
            "ping" => {
                let mut e = Envelope::new("pong");
                e.n = m.n;
                self.outbox.push(e);
            }
            "peer" => self.peer(b),
            "holds" => {
                if let Some(Value::Map(set)) = b.get("set") {
                    for (k, c) in set {
                        if let Some(c) = c.as_u64() {
                            self.holds.insert(k.clone(), c);
                        }
                    }
                }
                for k in b.get("clear").map(Value::as_arr).unwrap_or(&[]).iter().filter_map(Value::as_str) {
                    self.holds.remove(k);
                    self.my_holds.remove(k);
                }
            }
            "locks" => {
                for k in b.get("add").map(Value::as_arr).unwrap_or(&[]).iter().filter_map(Value::as_str) {
                    self.locks.insert(k.to_string());
                }
                for k in b.get("del").map(Value::as_arr).unwrap_or(&[]).iter().filter_map(Value::as_str) {
                    self.locks.remove(k);
                }
            }
            "saved" => {
                self.saved = (b.get("q").and_then(Value::as_u64).unwrap_or(0), s_of(b.get("hash")));
                self.events.push(DocEvent::Saved { q: self.saved.0 });
            }
            "join" => self.events.push(DocEvent::Join { c: b.get("c").and_then(Value::as_u64).unwrap_or(0), name: s_of(b.get("name")) }),
            "code" => self.events.push(DocEvent::Code { code: s_of(b.get("code")) }),
            "validate" => {
                let ops = b.get("ops").cloned().unwrap_or(Value::Nil);
                let mut reason = a.validate(&ops);
                if reason.is_none() && a.apply(&ops, "remote").is_err() {
                    reason = Some("invalid".into());
                }
                let c = b.get("c").cloned().unwrap_or(Value::Nil);
                let n = b.get("n").cloned().unwrap_or(Value::Nil);
                let body = match reason {
                    None => Value::map(vec![("c", c), ("n", n), ("ok", Value::Bool(true))]),
                    Some(r) => Value::map(vec![("c", c), ("n", n), ("ok", Value::Bool(false)), ("reason", Value::Str(r))]),
                };
                self.outbox.push(Envelope::new("valid").body(body));
            }
            "snapreq" => {
                let for_c = b.get("for").cloned().unwrap_or(Value::Nil);
                self.rollback_pending(a);
                let data = a.snapshot();
                self.reapply_pending(a);
                let (data, enc) =
                    if data.len() > 64 * 1024 { (miniz_oxide::deflate::compress_to_vec_zlib(&data, 6), "deflate") } else { (data, "") };
                self.outbox.push(Envelope::new("snap").body(Value::map(vec![
                    ("for", for_c),
                    ("q", Value::from(self.q)),
                    ("data", Value::Bin(data)),
                    ("enc", Value::from(enc)),
                ])));
            }
            _ => {}
        }
    }

    fn welcome(&mut self, m: &Envelope) {
        let b = &m.b;
        let first = !self.ever_welcomed;
        self.c = b.get("c").and_then(Value::as_u64).unwrap_or(0);
        self.role = s_of(b.get("role"));
        self.prefix = s_of(b.get("prefix"));
        self.token = s_of(b.get("token"));
        self.session = s_of(b.get("session"));
        if let Some(sv) = b.get("saved") {
            self.saved = (sv.get("q").and_then(Value::as_u64).unwrap_or(0), s_of(sv.get("hash")));
        }
        self.holds = match b.get("holds") {
            Some(Value::Map(h)) => h.iter().filter_map(|(k, v)| v.as_u64().map(|c| (k.clone(), c))).collect(),
            _ => HashMap::new(),
        };
        self.locks = b.get("locks").map(Value::as_arr).unwrap_or(&[]).iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
        if first {
            self.peers.clear();
        }
        for p in b.get("peers").map(Value::as_arr).unwrap_or(&[]).iter().take(1000) {
            let c = p.get("c").and_then(Value::as_u64).unwrap_or(0);
            self.peers.insert(c, Peer { c, name: s_of(p.get("name")), role: s_of(p.get("role")), state: "in".into() });
        }
        self.welcomed = true;
        self.ever_welcomed = true;
        let catchup = s_of(b.get("catchup"));
        if catchup == "snapshot" {
            self.snap = Some((Vec::new(), 0, 0, String::new()));
            self.loading = true;
        } else if first && catchup == "log" && self.q == 0 {
            self.q = self.saved.0;
        }
        self.set_status(if catchup == "snapshot" { "catching-up" } else { "live" });
        self.events.push(DocEvent::Welcome { c: self.c, role: self.role.clone() });
        let again: Vec<Envelope> = self.pending.iter().map(edit_env).collect();
        self.outbox.extend(again);
        if !self.my_holds.is_empty() {
            let ks: Vec<Value> = self.my_holds.iter().map(|k| Value::Str(k.clone())).collect();
            self.outbox.push(Envelope::new("hold").body(Value::map(vec![("keys", Value::Arr(ks))])));
        }
    }

    fn peer(&mut self, b: &Value) {
        let c = b.get("c").and_then(Value::as_u64).unwrap_or(0);
        let st = s_of(b.get("state"));
        let p = self.peers.entry(c).or_insert_with(|| Peer { c, ..Default::default() });
        if st == "role" {
            p.role = s_of(b.get("role"));
        } else {
            p.state = st.clone();
        }
        if let Some(n) = b.get("name").and_then(Value::as_str) {
            p.name = n.to_string();
        }
        if let Some(r) = b.get("role").and_then(Value::as_str) {
            p.role = r.to_string();
        }
        if c == self.c && st == "role" {
            self.role = s_of(b.get("role"));
        }
        self.events.push(DocEvent::Peer { c });
    }

    fn snap_chunk(&mut self, a: &mut dyn Adapter, m: &Envelope) {
        let b = &m.b;
        let i = usize::try_from(b.get("i").and_then(Value::as_u64).unwrap_or(0)).unwrap_or(0);
        let of = usize::try_from(b.get("of").and_then(Value::as_u64).unwrap_or(0)).unwrap_or(0);
        if i == 0 {
            if of == 0 {
                self.snap = None;
                self.loading = false;
                self.q = m.q;
                self.set_status("live");
                return;
            }
            // at most 4096 chunks (1 GB)
            self.snap = Some((vec![None; of.min(4096)], of.min(4096), m.q, s_of(b.get("enc"))));
        }
        let Some(s) = self.snap.as_mut() else { return };
        if let (Some(slot), Some(Value::Bin(d))) = (s.0.get_mut(i), b.get("data")) {
            *slot = Some(d.clone());
        }
        if s.0.iter().any(Option::is_none) {
            return;
        }
        let Some((parts, _, q, enc)) = self.snap.take() else { return };
        let bytes: Vec<u8> = parts.into_iter().flatten().flatten().collect();
        let data = if enc == "deflate" { inflate(&bytes) } else { Some(bytes) };
        let Some(data) = data else {
            self.set_status("failed");
            return;
        };
        for p in &mut self.pending {
            p.applied = false;
        }
        if a.load(&data, q).is_err() {
            self.set_status("failed");
            return;
        }
        self.q = q;
        self.reapply_pending(a);
        self.loading = false;
        self.set_status("live");
    }

    /// Apply what came in (live-client.js `flush`, once per frame).
    pub fn flush(&mut self, a: &mut dyn Adapter) {
        if self.loading || self.inbox.is_empty() {
            return;
        }
        let batch: Vec<In> = std::mem::take(&mut self.inbox);
        let mut applied = 0usize;
        let fast = batch.iter().enumerate().all(|(i, x)| match (x, self.pending.get(i)) {
            (In::Commit { c, n, .. }, Some(p)) => *c == self.c && p.n == *n && p.applied,
            _ => false,
        });
        if fast {
            for x in batch {
                let In::Commit { q, label, ops, touched, .. } = x else { continue };
                if !self.in_order(q) {
                    continue;
                }
                if self.pending.is_empty() {
                    break;
                }
                let p = self.pending.remove(0);
                self.track(q, 0, &touched);
                self.record(&label, ops, p.inverse.clone(), touched, q, Some(&p));
                applied = applied.saturating_add(1);
            }
        } else {
            self.rollback_pending(a);
            let mut ended = None;
            for x in batch {
                match x {
                    In::End { reason } => ended = Some(reason),
                    In::Refuse { n, reason, by, by_name } => self.refused(n, reason, by, by_name),
                    In::Commit { q, c, n, label, ops, touched } => {
                        if !self.in_order(q) {
                            continue;
                        }
                        let own = c == self.c;
                        let inv = a.invert(&ops).unwrap_or(Value::Nil);
                        let _ = a.apply(&ops, if own { "own" } else { "remote" });
                        self.track(q, c, &touched);
                        applied = applied.saturating_add(1);
                        if own {
                            let p = self.pending.iter().position(|p| p.n == n).map(|i| self.pending.remove(i));
                            self.record(&label, ops, inv, touched, q, p.as_ref());
                        }
                    }
                }
            }
            self.reapply_pending(a);
            if let Some(r) = ended {
                self.set_status("ended");
                self.events.push(DocEvent::Ended { reason: r });
            }
        }
        self.events.push(DocEvent::Applied { commits: applied });
    }

    fn in_order(&mut self, q: u64) -> bool {
        if q <= self.q {
            return false;
        }
        if q != self.q.saturating_add(1) {
            // a gap: the connection is no good (the caller reconnects)
            self.set_status("reconnecting");
            return false;
        }
        true
    }

    fn track(&mut self, q: u64, c: u64, touched: &[TouchReq]) {
        self.q = q;
        for t in touched {
            if t.m == 'r' {
                continue;
            }
            self.revs.insert(t.k.clone(), q);
            if let ('p', Some(f)) = (t.m, &t.f) {
                self.prop_revs.insert((t.k.clone(), f.clone()), (q, c));
            }
            if t.m == 'c' {
                self.prop_revs.retain(|(k, _), _| *k != t.k);
            }
            if t.m == 'd' {
                self.my_holds.remove(&t.k);
            }
        }
    }

    fn rollback_pending(&mut self, a: &mut dyn Adapter) {
        for p in self.pending.iter_mut().rev() {
            if !p.applied {
                continue;
            }
            let _ = a.apply(&p.inverse, "rollback");
            p.applied = false;
        }
    }

    fn reapply_pending(&mut self, a: &mut dyn Adapter) {
        for p in &mut self.pending {
            match a.invert(&p.ops) {
                Ok(inv) if a.apply(&p.ops, "rebase").is_ok() => {
                    p.inverse = inv;
                    p.applied = true;
                }
                _ => p.applied = false,
            }
        }
    }

    fn refused(&mut self, n: u64, reason: String, by: Option<u64>, by_name: String) {
        let Some(i) = self.pending.iter().position(|p| p.n == n) else { return };
        let p = self.pending.remove(i);
        let by_name =
            if by_name.is_empty() { by.and_then(|b| self.peers.get(&b)).map(|p| p.name.clone()).unwrap_or_default() } else { by_name };
        let undo = p.kind != "do";
        self.events.push(DocEvent::Refused { label: p.label.clone(), reason, by, by_name: by_name.clone(), undo });
        if undo {
            self.events.push(DocEvent::UndoSkipped { label: p.of.map_or(p.label, |e| e.label), by_name });
        }
    }

    fn record(&mut self, label: &str, ops: Value, inverse: Value, touched: Vec<TouchReq>, q: u64, p: Option<&Pending>) {
        let kind = p.map_or("do", |p| p.kind);
        let of_label = p.and_then(|p| p.of.as_ref()).map(|e| e.label.clone());
        let _ = ops;
        let mut entry = Entry { label: label.to_string(), inverse, touched, q };
        if kind == "undo" {
            entry.label = of_label.unwrap_or(entry.label);
            self.redo_stack.push(entry);
        } else {
            if kind == "redo" {
                entry.label = of_label.unwrap_or(entry.label);
            }
            self.undo_stack.push(entry);
            if self.undo_stack.len() > 500 {
                self.undo_stack.remove(0);
            }
        }
    }

    /// The app finished a step (applied already): numbered, kept pending, sent once welcomed. None for a viewer (the
    /// step is put back).
    pub fn local_change(&mut self, a: &mut dyn Adapter, label: &str, ops: Value, inverse: Value) -> Option<u64> {
        if self.role == "viewer" {
            let _ = a.apply(&inverse, "rollback");
            self.events.push(DocEvent::Refused {
                label: label.into(),
                reason: "role".into(),
                by: None,
                by_name: String::new(),
                undo: false,
            });
            return None;
        }
        let mut touched = a.touched(&ops);
        for t in &mut touched {
            if t.r == Cond::Auto && t.m == 's' {
                t.r = Cond::At(self.revs.get(&t.k).copied().unwrap_or(0));
            }
        }
        self.n = self.n.saturating_add(1);
        let p = Pending { n: self.n, label: label.to_string(), ops, inverse, touched, applied: true, kind: "do", of: None };
        if self.welcomed {
            self.outbox.push(edit_env(&p));
        }
        self.pending.push(p);
        self.redo_stack.clear();
        Some(self.n)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
    /// The label undo would take back.
    pub fn undo_label(&self) -> Option<String> {
        self.undo_stack.last().map(|e| e.label.clone())
    }
    pub fn redo_label(&self) -> Option<String> {
        self.redo_stack.last().map(|e| e.label.clone())
    }
    pub fn undo(&mut self, a: &mut dyn Adapter) -> bool {
        self.reverse(a, "undo")
    }
    pub fn redo(&mut self, a: &mut dyn Adapter) -> bool {
        self.reverse(a, "redo")
    }

    fn reverse(&mut self, a: &mut dyn Adapter, kind: &'static str) -> bool {
        let e = match kind {
            "undo" => self.undo_stack.pop(),
            _ => self.redo_stack.pop(),
        };
        let Some(e) = e else { return false };
        // a property I wrote that someone wrote over since: skip, and say who
        for t in e.touched.iter().filter(|t| t.m == 'p') {
            let Some(f) = &t.f else { continue };
            if let Some((cq, cc)) = self.prop_revs.get(&(t.k.clone(), f.clone()))
                && *cq != e.q
            {
                let by_name = self.peers.get(cc).map(|p| p.name.clone()).unwrap_or_default();
                self.events.push(DocEvent::UndoSkipped { label: e.label.clone(), by_name });
                return false;
            }
        }
        let ops = e.inverse.clone();
        let mut touched = a.touched(&ops);
        let mine: HashSet<String> = e.touched.iter().filter(|t| t.m != 'r').map(|t| t.k.clone()).collect();
        for t in &mut touched {
            if t.r != Cond::Auto {
                continue;
            }
            t.r = match t.m {
                'p' if mine.contains(&t.k) => Cond::At(e.q),
                'p' => Cond::None,
                's' | 'd' if mine.contains(&t.k) => Cond::At(e.q),
                's' | 'd' => Cond::At(self.revs.get(&t.k).copied().unwrap_or(0)),
                _ => Cond::None,
            };
        }
        for t in &mut touched {
            if t.m == 'p'
                && mine.contains(&t.k)
                && let Some(f) = &t.f
            {
                t.r = Cond::At(self.prop_revs.get(&(t.k.clone(), f.clone())).map_or(0, |x| x.0));
            }
        }
        let Ok(inverse) = a.invert(&ops) else {
            self.events.push(DocEvent::UndoSkipped { label: e.label.clone(), by_name: String::new() });
            return false;
        };
        if a.apply(&ops, kind).is_err() {
            self.events.push(DocEvent::UndoSkipped { label: e.label.clone(), by_name: String::new() });
            return false;
        }
        self.n = self.n.saturating_add(1);
        let label = format!("{} {}", if kind == "undo" { "Undo" } else { "Redo" }, e.label);
        let p = Pending { n: self.n, label, ops, inverse, touched, applied: true, kind, of: Some(e) };
        if self.welcomed {
            self.outbox.push(edit_env(&p));
        }
        self.pending.push(p);
        true
    }

    /// Hold keys (select, grips, command start), as `hold`.
    pub fn hold(&mut self, keys: &[String]) {
        for k in keys {
            self.my_holds.insert(k.clone());
        }
        let ks: Vec<Value> = keys.iter().map(|k| Value::Str(k.clone())).collect();
        self.outbox.push(Envelope::new("hold").body(Value::map(vec![("keys", Value::Arr(ks))])));
    }

    /// Is a pending edit still waiting?
    pub fn settled(&self) -> bool {
        self.pending.is_empty() && self.inbox.is_empty() && !self.loading
    }

    /// The labels of the pending edits (for tests and the status line).
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }
}

fn edit_env(p: &Pending) -> Envelope {
    let mut e = Envelope::new("edit").body(Value::map(vec![
        ("label", Value::Str(p.label.clone())),
        ("ops", p.ops.clone()),
        ("touched", touched_value(&p.touched)),
    ]));
    e.n = p.n;
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A toy model: a map of key → number; ops `[[key, value | nil]]`.
    #[derive(Default)]
    struct Toy(BTreeMap<String, i128>);
    impl Adapter for Toy {
        fn apply(&mut self, ops: &Value, _o: &str) -> Result<(), String> {
            for op in ops.as_arr() {
                let k = op.as_arr().first().and_then(Value::as_str).ok_or("op")?.to_string();
                match op.as_arr().get(1) {
                    Some(Value::Int(v)) => {
                        self.0.insert(k, *v);
                    }
                    _ => {
                        self.0.remove(&k);
                    }
                }
            }
            Ok(())
        }
        fn invert(&mut self, ops: &Value) -> Result<Value, String> {
            let mut out = Vec::new();
            for op in ops.as_arr().iter().rev() {
                let k = op.as_arr().first().and_then(Value::as_str).ok_or("op")?.to_string();
                out.push(Value::Arr(vec![Value::Str(k.clone()), self.0.get(&k).map_or(Value::Nil, |v| Value::Int(*v))]));
            }
            Ok(Value::Arr(out))
        }
        fn touched(&self, ops: &Value) -> Vec<TouchReq> {
            ops.as_arr()
                .iter()
                .filter_map(|op| op.as_arr().first().and_then(Value::as_str))
                .map(|k| TouchReq { k: format!("e:{k}"), m: 'p', f: Some("v".into()), r: Cond::None, g: false })
                .collect()
        }
        fn snapshot(&mut self) -> Vec<u8> {
            Vec::new()
        }
        fn load(&mut self, _b: &[u8], _q: u64) -> Result<(), String> {
            Ok(())
        }
    }

    fn op(k: &str, v: i128) -> Value {
        Value::Arr(vec![Value::Arr(vec![Value::Str(k.into()), Value::Int(v)])])
    }
    fn commit(q: u64, c: u64, n: u64, ops: Value) -> Envelope {
        let mut e = Envelope::new("commit").body(Value::map(vec![
            ("c", Value::from(c)),
            ("n", Value::from(n)),
            ("label", Value::from("x")),
            ("ops", ops),
            ("touched", Value::Arr(vec![Value::map(vec![("k", Value::from("e:a")), ("m", Value::from("p")), ("f", Value::from("v"))])])),
        ]));
        e.q = q;
        e.c = c;
        e.n = n;
        e
    }

    #[test]
    fn a_pending_edit_rebases_over_a_remote_commit_and_a_refusal_takes_it_back() {
        let mut m = Toy::default();
        let mut d = LiveDoc::new();
        let w = Envelope::new("welcome").body(Value::map(vec![
            ("c", Value::from(1u64)),
            ("role", Value::from("editor")),
            ("catchup", Value::from("log")),
        ]));
        d.message(&mut m, &w);
        assert!(d.is_live());
        // mine: a=1 (applied by the app already)
        let inv = m.invert(&op("a", 1)).unwrap();
        m.apply(&op("a", 1), "own").unwrap();
        let n = d.local_change(&mut m, "set a", op("a", 1), inv).unwrap();
        assert_eq!(d.outbox.len(), 1);
        // someone else's b=5 arrives first: mine is rolled back, theirs applied, mine re-applied on top
        d.message(&mut m, &commit(1, 2, 1, op("b", 5)));
        d.flush(&mut m);
        assert_eq!((m.0.get("a"), m.0.get("b")), (Some(&1), Some(&5)));
        // mine is refused: taken back
        let mut r = Envelope::new("refuse").body(Value::map(vec![("n", Value::from(n)), ("reason", Value::from("locked"))]));
        r.n = n;
        d.message(&mut m, &r);
        d.flush(&mut m);
        assert_eq!(m.0.get("a"), None);
        assert!(d.events.iter().any(|e| matches!(e, DocEvent::Refused { reason, .. } if reason == "locked")));
        assert!(d.settled());
        // a commit out of order is a gap
        d.message(&mut m, &commit(5, 2, 2, op("b", 6)));
        d.flush(&mut m);
        assert_eq!(d.status, "reconnecting");
    }

    #[test]
    fn own_commit_is_recorded_for_undo_and_undo_goes_out_as_an_edit() {
        let mut m = Toy::default();
        let mut d = LiveDoc::new();
        let w = Envelope::new("welcome").body(Value::map(vec![
            ("c", Value::from(3u64)),
            ("role", Value::from("host")),
            ("catchup", Value::from("log")),
        ]));
        d.message(&mut m, &w);
        let inv = m.invert(&op("a", 7)).unwrap();
        m.apply(&op("a", 7), "own").unwrap();
        d.local_change(&mut m, "seven", op("a", 7), inv);
        d.message(&mut m, &commit(1, 3, 1, op("a", 7)));
        d.flush(&mut m);
        assert!(d.can_undo());
        d.outbox.clear();
        assert!(d.undo(&mut m));
        assert_eq!(m.0.get("a"), None);
        assert_eq!(d.outbox.len(), 1);
        assert_eq!(d.outbox[0].b.get("label").and_then(Value::as_str), Some("Undo x"), "the label is the commit's (live-client.js record)");
    }
}
