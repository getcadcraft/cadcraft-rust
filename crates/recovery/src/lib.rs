//! CadCraft L5: recovery copies of unsaved work — 0.5's shell `app-autosave.js` (the `Autosave` class), without its
//! dialog (the window draws that: ui-egui `recoverdlg.rs`).
//!
//! A copy is written a moment after edits stop ([`Opts::delay`], 2 s) and at least every [`Opts::max_wait`] (30 s)
//! while they go on; a document saved to a file (or undone back to it) drops its copy, a closed one too. At start the
//! copies left by sessions that are no longer running are offered back ([`Autosave::orphans`]); copies older than
//! [`Opts::keep_days`] (30) or beyond the newest [`Opts::keep_per_app`] (20) are removed then.
//!
//! Nothing here reads a clock or a timer: the host passes `now` (ms since 1970) to every call and asks [`Autosave::due`]
//! which documents want their copy written, so tests and the parity harness run on a fixed clock. Storage is a
//! key-value [`Service`] the host gives (CLAUDE.md § 6 Recovery): a per-instance locked folder on the desktop
//! ([`dir::DirKv`]), browser storage on the web, [`memory`] in tests. Keys: `i:<key>` an index record (JSON), `d:<key>`
//! its data, `b:<app>:<session>` a heartbeat. Which sessions still run: the store's locks ([`RecoveryOp::Live`]) when it
//! has them, else heartbeats (as 0.5 without Web Locks).
//!
//! Everything read back is hostile (another version, a damaged disk, a person's edits): a record that does not parse is
//! skipped and counted in [`Autosave::damaged`], sizes are capped, nothing panics.

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]

#[cfg(not(target_arch = "wasm32"))]
pub mod dir;

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::{Map, Value};

/// One request to the store (`Services::recovery`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecoveryOp {
    /// Is the store ready (the web loads browser storage asynchronously)? `Some` = ready.
    Ready,
    /// This session starts: hold its lock (stores with locks).
    Start(String),
    /// This session stops: release its lock.
    Stop(String),
    Put(String, Vec<u8>),
    /// `None` = no such key
    Get(String),
    Delete(String),
    /// Every key, one per line.
    List,
    /// The sessions running now, one per line (from the store's locks); `None` = the store has no locks (heartbeats).
    Live,
}

/// The store: answers one [`RecoveryOp`] (an error is a message; "quota" in it = the storage is full).
pub type Service = Box<dyn FnMut(RecoveryOp) -> Result<Option<Vec<u8>>, String>>;

/// The record format (0.5 `FORMAT`).
pub const FORMAT: f64 = 1.0;
/// An index record is small; a bigger one is not read.
pub const MAX_INDEX_BYTES: usize = 1 << 20;
/// A copy's data is not read past this (a damaged or planted file).
pub const MAX_DATA_BYTES: usize = 1 << 30;
/// Keys looked at in one listing.
pub const MAX_KEYS: usize = 100_000;

/// 0.5 `DEFAULTS` (ms, days, count).
#[derive(Clone, Debug, PartialEq)]
pub struct Opts {
    pub delay: f64,
    pub max_wait: f64,
    pub keep_days: f64,
    pub keep_per_app: usize,
    pub beat: f64,
    pub stale: f64,
}

impl Default for Opts {
    fn default() -> Self {
        Opts { delay: 2000.0, max_wait: 30000.0, keep_days: 30.0, keep_per_app: 20, beat: 15000.0, stale: 45000.0 }
    }
}

/// A copy as the dialog lists it (0.5 `publicRec`).
#[derive(Clone, Debug, PartialEq)]
pub struct Rec {
    pub key: String,
    pub app: String,
    pub session: String,
    pub doc: String,
    pub name: String,
    pub saved_at: f64,
    pub size: f64,
    pub meta: Value,
    pub format: f64,
}

impl Rec {
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("key".into(), Value::String(self.key.clone()));
        m.insert("format".into(), num(self.format));
        m.insert("app".into(), Value::String(self.app.clone()));
        m.insert("session".into(), Value::String(self.session.clone()));
        m.insert("doc".into(), Value::String(self.doc.clone()));
        m.insert("name".into(), Value::String(self.name.clone()));
        m.insert("savedAt".into(), num(self.saved_at));
        m.insert("size".into(), num(self.size));
        m.insert("meta".into(), self.meta.clone());
        Value::Object(m)
    }
    /// A record read back (None: not one).
    pub fn from_json(v: &Value) -> Option<Rec> {
        let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        let n = |k: &str| v.get(k).and_then(Value::as_f64).filter(|x| x.is_finite());
        Some(Rec {
            key: s("key")?,
            app: s("app")?,
            session: s("session")?,
            doc: s("doc")?,
            name: s("name").unwrap_or_default(),
            saved_at: n("savedAt")?,
            size: n("size").unwrap_or(0.0).max(0.0),
            meta: v.get("meta").cloned().unwrap_or(Value::Null),
            format: n("format").unwrap_or(FORMAT),
        })
    }
}

/// A JSON number: integral values as integers (as JS writes them).
fn num(x: f64) -> Value {
    if x.is_finite() && x.fract() == 0.0 && x.abs() < 9.0e15 {
        // integral and in range
        #[allow(clippy::cast_possible_truncation)]
        let i = x as i64;
        Value::from(i)
    } else {
        serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number)
    }
}

/// `app + ':' + session + ':' + doc`
pub fn rec_key(app: &str, session: &str, doc: &str) -> String {
    format!("{app}:{session}:{doc}")
}

/// A new session id: 32 hex digits (0.5 `newId()`: a random UUID).
pub fn new_session_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::with_capacity(32);
    for k in 0..2u64 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(k);
        h.write_u128(std::time::UNIX_EPOCH.elapsed().map_or(0, |d| d.as_nanos()));
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

/// Why a write failed: the storage is full, or anything else (0.5 codes `quota` / `write`); `unavailable` = no storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    Quota,
    Write,
    Unavailable,
}

impl Problem {
    /// The shell's message key.
    pub fn key(self) -> &'static str {
        match self {
            Problem::Quota => "shell.autosave.err.quota",
            Problem::Write => "shell.autosave.err.write",
            Problem::Unavailable => "shell.autosave.err.unavailable",
        }
    }
    fn of(msg: &str) -> Problem {
        if msg.to_ascii_lowercase().contains("quota") { Problem::Quota } else { Problem::Write }
    }
}

#[derive(Clone, Debug)]
struct DocState {
    id: String,
    name: String,
    dirty: bool,
    first_dirty: Option<f64>,
    due: Option<f64>,
    saved_at: Option<f64>,
}

/// One app's recovery copies for this session (0.5 `Autosave` + `AutosaveDoc`).
pub struct Autosave {
    pub app: String,
    pub session: String,
    pub opts: Opts,
    svc: Service,
    docs: Vec<DocState>,
    /// problems to say once each (the window shows them, then clears the list)
    pub said: Vec<Problem>,
    warned: Vec<Problem>,
    /// every failed write since start (0.5 `onError`)
    pub errors: Vec<(Problem, String)>,
    /// keys of records that could not be read (skipped, kept)
    pub damaged: Vec<String>,
    locks: bool,
    last_beat: Option<f64>,
    /// tests: the sessions running now (overrides locks and heartbeats; 0.5 `alive`)
    pub alive: Option<Box<dyn Fn() -> Vec<String>>>,
    started: bool,
}

impl std::fmt::Debug for Autosave {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Autosave").field("app", &self.app).field("session", &self.session).field("docs", &self.docs.len()).finish()
    }
}

impl Autosave {
    pub fn new(app: &str, session: &str, svc: Service) -> Autosave {
        Autosave {
            app: app.to_string(),
            session: session.to_string(),
            opts: Opts::default(),
            svc,
            docs: Vec::new(),
            said: Vec::new(),
            warned: Vec::new(),
            errors: Vec::new(),
            damaged: Vec::new(),
            locks: false,
            last_beat: None,
            alive: None,
            started: false,
        }
    }

    fn op(&mut self, op: RecoveryOp) -> Result<Option<Vec<u8>>, String> {
        (self.svc)(op)
    }

    /// Say a problem once (and keep it for the host).
    pub fn warn(&mut self, p: Problem, msg: &str) {
        self.errors.push((p, msg.to_string()));
        if !self.warned.contains(&p) {
            self.warned.push(p);
            self.said.push(p);
        }
    }

    /// Hold this session's lock (or write its first heartbeat). → false when the store cannot be used (said once).
    pub fn start(&mut self, now: f64) -> bool {
        if self.started {
            return true;
        }
        self.started = true;
        let s = self.session.clone();
        if let Err(e) = self.op(RecoveryOp::Start(s)) {
            self.warn(Problem::Unavailable, &e);
            return false;
        }
        match self.op(RecoveryOp::Live) {
            Ok(Some(_)) => self.locks = true,
            Ok(None) => {
                if self.alive.is_none() {
                    self.beat(now);
                }
            }
            Err(e) => {
                self.warn(Problem::Unavailable, &e);
                return false;
            }
        }
        true
    }

    fn beat(&mut self, now: f64) {
        self.last_beat = Some(now);
        let mut m = Map::new();
        m.insert("app".into(), Value::String(self.app.clone()));
        m.insert("session".into(), Value::String(self.session.clone()));
        m.insert("beat".into(), num(now));
        let key = format!("b:{}:{}", self.app, self.session);
        let _ = self.op(RecoveryOp::Put(key, Value::Object(m).to_string().into_bytes()));
    }

    /// Release the lock / the heartbeat (the copies stay and become recoverable). Pending writes are dropped.
    pub fn stop(&mut self) {
        for d in &mut self.docs {
            d.due = None;
        }
        let s = self.session.clone();
        let _ = self.op(RecoveryOp::Stop(s));
        if !self.locks && self.alive.is_none() {
            let key = format!("b:{}:{}", self.app, self.session);
            let _ = self.op(RecoveryOp::Delete(key));
        }
        self.started = false;
    }

    /// Time passes: heartbeats, and the documents whose copy is due now (write each with [`Autosave::save`]).
    pub fn due(&mut self, now: f64) -> Vec<String> {
        if self.started && !self.locks && self.alive.is_none() && self.last_beat.is_none_or(|b| now - b >= self.opts.beat) {
            self.beat(now);
        }
        let mut out = Vec::new();
        for d in &mut self.docs {
            if d.due.is_some_and(|t| t <= now) {
                d.due = None;
                out.push(d.id.clone());
            }
        }
        out
    }

    /// When the next copy is due (the host wakes up then).
    pub fn next_due(&self) -> Option<f64> {
        self.docs.iter().filter_map(|d| d.due).reduce(f64::min)
    }

    fn find(&mut self, id: &str) -> Option<&mut DocState> {
        self.docs.iter_mut().find(|d| d.id == id)
    }

    /// A document to keep a copy of (again: renamed).
    pub fn doc(&mut self, id: &str, name: &str) {
        if let Some(d) = self.find(id) {
            d.name = name.to_string();
            return;
        }
        self.docs.push(DocState { id: id.to_string(), name: name.to_string(), dirty: false, first_dirty: None, due: None, saved_at: None });
    }

    pub fn has_doc(&self, id: &str) -> bool {
        self.docs.iter().any(|d| d.id == id)
    }

    /// The documents with edits not yet in their copy.
    pub fn dirty_docs(&self) -> Vec<String> {
        self.docs.iter().filter(|d| d.dirty).map(|d| d.id.clone()).collect()
    }

    /// After every edit (0.5 `changed()`).
    pub fn changed(&mut self, id: &str, now: f64) {
        let (delay, max_wait) = (self.opts.delay, self.opts.max_wait);
        let Some(d) = self.find(id) else { return };
        d.dirty = true;
        let first = *d.first_dirty.get_or_insert(now);
        let wait = delay.min(first + max_wait - now).max(0.0);
        d.due = Some(now + wait);
    }

    /// Write a document's copy now: `snap` = (data, meta), or None when there is nothing to keep (the document is not
    /// dirty any more). → true when written. A failed write is said and tried again after `max_wait`.
    pub fn save(&mut self, id: &str, snap: Option<(Vec<u8>, Value)>, now: f64) -> bool {
        let max_wait = self.opts.max_wait;
        let (app, session) = (self.app.clone(), self.session.clone());
        let Some(d) = self.find(id) else { return false };
        if !d.dirty {
            return false;
        }
        d.due = None;
        let Some((data, meta)) = snap else {
            d.dirty = false;
            d.first_dirty = None;
            return false;
        };
        let key = rec_key(&app, &session, id);
        let rec = Rec {
            key: key.clone(),
            app,
            session,
            doc: id.to_string(),
            name: d.name.clone(),
            saved_at: now,
            size: data.len() as f64,
            meta,
            format: FORMAT,
        };
        let r = self
            .op(RecoveryOp::Put(format!("d:{key}"), data))
            .and_then(|_| self.op(RecoveryOp::Put(format!("i:{key}"), rec.to_json().to_string().into_bytes())));
        match r {
            Ok(_) => {
                if let Some(d) = self.find(id) {
                    d.saved_at = Some(now);
                    d.dirty = false;
                    d.first_dirty = None;
                }
                true
            }
            Err(e) => {
                self.warn(Problem::of(&e), &e);
                if let Some(d) = self.find(id) {
                    d.due = Some(now + max_wait);
                }
                false
            }
        }
    }

    /// The document matches a file again (saved, or undone back): its copy goes (0.5 `clean()`).
    pub fn clean(&mut self, id: &str) {
        let key = rec_key(&self.app, &self.session, id);
        if let Some(d) = self.find(id) {
            d.due = None;
            d.dirty = false;
            d.first_dirty = None;
            d.saved_at = None;
        }
        let _ = self.op(RecoveryOp::Delete(format!("i:{key}")));
        let _ = self.op(RecoveryOp::Delete(format!("d:{key}")));
    }

    /// Closed or discarded on purpose: the copy goes and the document is forgotten.
    pub fn close(&mut self, id: &str) {
        self.clean(id);
        self.docs.retain(|d| d.id != id);
    }

    /// Stop following a document but keep its copy (it is offered back once this session ends).
    pub fn forget(&mut self, id: &str) {
        self.docs.retain(|d| d.id != id);
    }

    /// A new name (the copy's record follows when there is one).
    pub fn rename(&mut self, id: &str, name: &str) {
        let key = rec_key(&self.app, &self.session, id);
        let Some(d) = self.find(id) else { return };
        if d.name == name {
            return;
        }
        d.name = name.to_string();
        if d.saved_at.is_none() {
            return;
        }
        if let Ok(Some(b)) = self.op(RecoveryOp::Get(format!("i:{key}")))
            && let Some(mut r) = parse_rec(&b)
        {
            r.name = name.to_string();
            let _ = self.op(RecoveryOp::Put(format!("i:{key}"), r.to_json().to_string().into_bytes()));
        }
    }

    fn keys(&mut self, prefix: &str) -> Vec<String> {
        let Ok(Some(b)) = self.op(RecoveryOp::List) else { return Vec::new() };
        String::from_utf8_lossy(&b).lines().filter(|k| k.starts_with(prefix)).take(MAX_KEYS).map(str::to_string).collect()
    }

    /// Every record of this app, in the store's order (damaged ones skipped and noted).
    pub fn index(&mut self) -> Vec<Rec> {
        let mut out = Vec::new();
        for k in self.keys("i:") {
            let Ok(Some(b)) = self.op(RecoveryOp::Get(k.clone())) else { continue };
            match parse_rec(&b) {
                Some(r) if k.get(2..) == Some(r.key.as_str()) => {
                    if r.app == self.app {
                        out.push(r);
                    }
                }
                _ => {
                    if !self.damaged.contains(&k) {
                        self.damaged.push(k);
                    }
                }
            }
        }
        out
    }

    /// The sessions running now (this one included).
    fn live(&mut self, now: f64) -> Vec<String> {
        let mut out = vec![self.session.clone()];
        if let Some(f) = &self.alive {
            out.extend(f());
            return out;
        }
        if self.locks {
            if let Ok(Some(b)) = self.op(RecoveryOp::Live) {
                out.extend(String::from_utf8_lossy(&b).lines().map(str::to_string));
            }
            return out;
        }
        for (session, beat) in self.beats() {
            if now - beat < self.opts.stale {
                out.push(session);
            }
        }
        out
    }

    fn beats(&mut self) -> Vec<(String, f64)> {
        let mut out = Vec::new();
        for k in self.keys(&format!("b:{}:", self.app)) {
            let Ok(Some(b)) = self.op(RecoveryOp::Get(k)) else { continue };
            let Some(v) = (b.len() <= MAX_INDEX_BYTES).then(|| serde_json::from_slice::<Value>(&b).ok()).flatten() else { continue };
            if let (Some(s), Some(t)) = (v.get("session").and_then(Value::as_str), v.get("beat").and_then(Value::as_f64)) {
                out.push((s.to_string(), t));
            }
        }
        out
    }

    /// Copies to offer back: written by sessions no longer running, newest first. Prunes copies older than `keep_days`
    /// or beyond `keep_per_app`, and heartbeats of stopped sessions (0.5 `orphans()`).
    pub fn orphans(&mut self, now: f64) -> Vec<Rec> {
        let all = self.index();
        let live = self.live(now);
        let max_age = self.opts.keep_days * 86_400_000.0;
        let mut dead: Vec<Rec> = all.into_iter().filter(|r| !live.contains(&r.session)).collect();
        // Array.prototype.sort (stable) with (a, b) => b.savedAt - a.savedAt
        dead.sort_by(|a, b| b.saved_at.partial_cmp(&a.saved_at).unwrap_or(std::cmp::Ordering::Equal));
        let mut keep = Vec::new();
        for r in dead {
            if now - r.saved_at > max_age || keep.len() >= self.opts.keep_per_app {
                let _ = self.op(RecoveryOp::Delete(format!("i:{}", r.key)));
                let _ = self.op(RecoveryOp::Delete(format!("d:{}", r.key)));
            } else {
                keep.push(r);
            }
        }
        if !self.locks && self.alive.is_none() {
            for (s, _) in self.beats() {
                if !live.contains(&s) {
                    let k = format!("b:{}:{s}", self.app);
                    let _ = self.op(RecoveryOp::Delete(k));
                }
            }
        }
        keep
    }

    /// One copy with its data (None: it is gone — another window recovered or discarded it; or its data is damaged).
    pub fn load(&mut self, key: &str) -> Option<(Rec, Vec<u8>)> {
        let rec = self.index().into_iter().find(|r| r.key == key)?;
        match self.op(RecoveryOp::Get(format!("d:{key}"))) {
            Ok(Some(d)) if d.len() <= MAX_DATA_BYTES => Some((rec, d)),
            _ => None,
        }
    }

    /// Remove a copy (recovered, or discarded by the person).
    pub fn discard(&mut self, key: &str) {
        let _ = self.op(RecoveryOp::Delete(format!("i:{key}")));
        let _ = self.op(RecoveryOp::Delete(format!("d:{key}")));
    }
}

fn parse_rec(b: &[u8]) -> Option<Rec> {
    if b.len() > MAX_INDEX_BYTES {
        return None;
    }
    Rec::from_json(&serde_json::from_slice::<Value>(b).ok()?)
}

/// The shared contents of a [`memory`] store: keys in insertion order (a re-put keeps its place, as a JS `Map`).
#[derive(Debug, Default)]
pub struct MemState {
    pub items: Vec<(String, Vec<u8>)>,
    /// sessions holding a lock; `None` = the store has no locks (heartbeats)
    pub locks: Option<Vec<String>>,
    /// every op fails with this message (tests: a full or broken store)
    pub fail: Option<String>,
}

/// An in-memory store (tests, the harness; 0.5 `memoryStore()`): the handle sees the same contents.
pub fn memory() -> (Service, Rc<RefCell<MemState>>) {
    let st: Rc<RefCell<MemState>> = Rc::default();
    (memory_on(Rc::clone(&st)), st)
}

/// A [`memory`] store over existing contents (two "windows" sharing one store).
pub fn memory_on(s2: Rc<RefCell<MemState>>) -> Service {
    Box::new(move |op| {
        let mut s = s2.borrow_mut();
        if let Some(e) = &s.fail
            && !matches!(op, RecoveryOp::Ready | RecoveryOp::Stop(_))
        {
            return Err(e.clone());
        }
        Ok(match op {
            RecoveryOp::Ready => Some(Vec::new()),
            RecoveryOp::Start(sess) => {
                if let Some(l) = s.locks.as_mut()
                    && !l.contains(&sess)
                {
                    l.push(sess);
                }
                None
            }
            RecoveryOp::Stop(sess) => {
                if let Some(l) = s.locks.as_mut() {
                    l.retain(|x| *x != sess);
                }
                None
            }
            RecoveryOp::Put(k, v) => {
                match s.items.iter_mut().find(|(kk, _)| *kk == k) {
                    Some(slot) => slot.1 = v,
                    None => s.items.push((k, v)),
                }
                None
            }
            RecoveryOp::Get(k) => s.items.iter().find(|(kk, _)| *kk == k).map(|(_, v)| v.clone()),
            RecoveryOp::Delete(k) => {
                s.items.retain(|(kk, _)| *kk != k);
                None
            }
            RecoveryOp::List => Some(s.items.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join("\n").into_bytes()),
            RecoveryOp::Live => s.locks.as_ref().map(|l| l.join("\n").into_bytes()),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn auto(app: &str, sess: &str, st: &Rc<RefCell<MemState>>) -> Autosave {
        Autosave::new(app, sess, memory_on(Rc::clone(st)))
    }

    #[test]
    fn cadence_delay_and_max_wait() {
        let st: Rc<RefCell<MemState>> = Rc::default();
        let mut a = auto("cadcraft", "s1", &st);
        a.alive = Some(Box::new(Vec::new));
        assert!(a.start(0.0));
        a.doc("tab1", "Plan");
        a.changed("tab1", 1000.0);
        assert!(a.due(2999.0).is_empty());
        assert_eq!(a.due(3000.0), vec!["tab1".to_string()]);
        assert!(a.save("tab1", Some((vec![1, 2, 3], json!({"name": "Plan"}))), 3000.0));
        // edits every second: the copy is written 30 s after the first unsaved one at the latest
        let mut writes = Vec::new();
        for s in 10..80 {
            let t = f64::from(s) * 1000.0;
            for id in a.due(t) {
                a.save(&id, Some((vec![0], json!({}))), t);
                writes.push(t);
            }
            a.changed("tab1", t);
        }
        assert_eq!(writes, vec![40000.0, 70000.0]);
    }

    #[test]
    fn orphans_live_stale_prune() {
        let st: Rc<RefCell<MemState>> = Rc::default();
        let day = 86_400_000.0;
        let mut a = auto("cadcraft", "s1", &st);
        a.start(0.0);
        for i in 0..23u32 {
            let id = format!("tab{i}");
            a.doc(&id, &id);
            a.changed(&id, 0.0);
            a.save(&id, Some((vec![7; 10], json!({}))), f64::from(i) * 1000.0);
        }
        a.doc("old", "old");
        a.changed("old", 0.0);
        a.save("old", Some((vec![1], json!({}))), -31.0 * day);
        // s1 is still beating: a second window sees nothing to offer
        let mut b = auto("cadcraft", "s2", &st);
        b.start(10_000.0);
        assert!(b.orphans(20_000.0).is_empty());
        // s1 stopped beating 45 s ago: its copies are offered, newest first, 20 at most; the old one is gone
        let o = b.orphans(10_000.0 + 46_000.0 + 22_000.0);
        assert_eq!(o.len(), 20);
        assert_eq!(o.first().map(|r| r.doc.as_str()), Some("tab22"));
        assert_eq!(b.index().len(), 20);
        let (r, d) = b.load(&o[0].key).unwrap();
        assert_eq!((r.size, d.len()), (10.0, 10));
        b.discard(&r.key);
        assert!(b.load(&r.key).is_none());
    }

    #[test]
    fn clean_close_rename_and_failures() {
        let st: Rc<RefCell<MemState>> = Rc::default();
        let mut a = auto("cadcraft", "s1", &st);
        a.alive = Some(Box::new(Vec::new));
        a.start(0.0);
        a.doc("t", "A");
        a.changed("t", 0.0);
        assert!(a.save("t", Some((vec![1], json!({}))), 2000.0));
        a.rename("t", "B");
        assert_eq!(a.index()[0].name, "B");
        a.clean("t");
        assert!(a.index().is_empty());
        // a full store: said once, retried after max_wait
        st.borrow_mut().fail = Some("QuotaExceededError: quota".into());
        a.changed("t", 3000.0);
        assert!(!a.save("t", Some((vec![1], json!({}))), 5000.0));
        assert_eq!(a.said, vec![Problem::Quota]);
        assert_eq!(a.next_due(), Some(35000.0));
        st.borrow_mut().fail = None;
        assert!(a.save("t", Some((vec![1], json!({}))), 35000.0));
        a.close("t");
        assert!(a.index().is_empty() && !a.has_doc("t"));
    }

    #[test]
    fn damaged_records_are_skipped() {
        let st: Rc<RefCell<MemState>> = Rc::default();
        st.borrow_mut().items.push(("i:cadcraft:x:t".into(), b"{not json".to_vec()));
        st.borrow_mut().items.push(("i:cadcraft:y:t".into(), vec![b'x'; MAX_INDEX_BYTES + 1]));
        st.borrow_mut()
            .items
            .push(("i:cadcraft:z:t".into(), br#"{"key":"other","app":"cadcraft","session":"z","doc":"t","savedAt":1}"#.to_vec()));
        st.borrow_mut().items.push((
            "i:cadcraft:w:t".into(),
            br#"{"key":"cadcraft:w:t","app":"cadcraft","session":"w","doc":"t","savedAt":1e400}"#.to_vec(),
        ));
        let mut a = auto("cadcraft", "s1", &st);
        a.alive = Some(Box::new(Vec::new));
        a.start(0.0);
        assert!(a.orphans(10.0).is_empty());
        assert_eq!(a.damaged.len(), 4);
        // a record with no data: load says it is gone
        st.borrow_mut()
            .items
            .push(("i:cadcraft:v:t".into(), br#"{"key":"cadcraft:v:t","app":"cadcraft","session":"v","doc":"t","savedAt":5}"#.to_vec()));
        assert_eq!(a.orphans(10.0).len(), 1);
        assert!(a.load("cadcraft:v:t").is_none());
    }
}
