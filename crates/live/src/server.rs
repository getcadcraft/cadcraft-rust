//! The session server's core (docs/STUDIO-LIVE-SPEC.md § 5, § 5a, § 6, § 7, § 11; LIVE-PROTOCOL.md version 1) — the
//! same decisions as Studio 0.5's `desktop-shell/src-tauri/src/live/session.rs`, ported to the fork's own MessagePack.
//!
//! A pure state machine: no sockets, no clock of its own, no threads. `net.rs` feeds it frames and the time and carries
//! out what it returns. Tests drive it directly.
//!
//! What it decides:
//! - who gets in: the PAKE handshake against the invite secrets (short codes, link keys, rejoin tickets, assistants'
//!   keys), wrong-try counting and code rotation; then hello → waiting → the host approves (a rejoin with the resume
//!   token skips approval); roles;
//! - the order: every edit gets the next `seq` or a `refuse`, all-or-nothing;
//! - property writes commit last-writer-wins (refused only on a deleted object, or when an undo's condition fails);
//!   structural ops commit only if every key they name is still at the version they were based on;
//! - holds (stop others' structural ops and guarded reads), locks (host only, stop every op, the host's too);
//! - the validator (a replica that holds exactly the committed state) before every commit;
//! - presence fan-out, pings every 2 s, 3 missed = gone, flood control, limits;
//! - catch-up from the log or a snapshot, `saved`, `end`, the host handoff's state.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use crate::crypto;
use crate::envelope::{Envelope, PROTOCOL};
use crate::msgpack::Value;

pub type ConnId = u64;
/// Milliseconds on the caller's monotonic clock.
pub type Ms = u64;

pub const PING_EVERY: Ms = 2_000;
pub const GONE_AFTER: Ms = 6_000; // 3 missed pings
/// Presence fan-out tick: each peer's newest presence goes out within one tick.
pub const PRESENCE_EVERY: Ms = 16;
pub const HOLD_TTL: Ms = 30_000;
pub const SNAPSHOT_CHUNK: usize = 256 * 1024;
/// Key versions per `revs` message sent before a snapshot.
pub const REVS_CHUNK: usize = 10_000;
pub const MAX_EDIT_BYTES: usize = 1024 * 1024;
pub const RATE_PER_SEC: f64 = 100.0;
pub const BACKLOG_WARN: usize = 200;
pub const BACKLOG_REMOVE: usize = 1000;
/// How long a dropped client's place (id, prefix, holds) is kept for a rejoin.
pub const REJOIN_WINDOW: Ms = 10 * 60_000;
/// A joiner that sent handshake message 1 must prove the code (message 3) within this, or it counts as a wrong try;
/// a connection that never says hello is closed after it too.
pub const HANDSHAKE_TIMEOUT: Ms = 10_000;
pub const CODE_TTL: Ms = 10 * 60_000;
pub const WRONG_TRIES: u32 = 3;
/// A rejoin ticket lasts as long as the session.
const TICKET_TTL: Ms = Ms::MAX / 4;
/// An assistant's one-time key: asked for by its person, used at once by the assistant's own connection.
pub const ASSISTANT_KEY_TTL: Ms = 60_000;
/// Assistants per person at a time (connected, waiting, or keys not used yet).
pub const MAX_ASSISTANTS: usize = 4;
/// Handoff state goes in chunks no bigger than this (the relay refuses frames over 2 MB).
pub const HANDOVER_CHUNK: usize = 256 * 1024;

/// A role (§ 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Host,
    Editor,
    Viewer,
    Assistant,
    /// The host's validator: holds exactly the committed state, answers `validate` and `snapreq`. Never a person.
    Replica,
}

impl Role {
    pub fn name(self) -> &'static str {
        match self {
            Role::Host => "host",
            Role::Editor => "editor",
            Role::Viewer => "viewer",
            Role::Assistant => "assistant",
            Role::Replica => "replica",
        }
    }
    pub fn parse(s: &str) -> Option<Role> {
        Some(match s {
            "host" => Role::Host,
            "editor" | "edit" => Role::Editor,
            "viewer" | "view" => Role::Viewer,
            "assistant" => Role::Assistant,
            "replica" => Role::Replica,
            _ => return None,
        })
    }
    fn can_edit(self) -> bool {
        matches!(self, Role::Host | Role::Editor | Role::Assistant)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Waiting,
    Active,
    Gone,
}

/// How the session runs.
#[derive(Debug, Clone)]
pub struct Config {
    pub app: String,
    pub schema: u32,
    /// People besides the host; one more is refused (`deny full`).
    pub max_clients: usize,
    /// How many commits the log keeps for rejoins and same-file joins; older ones mean a snapshot.
    pub log_keep: usize,
    /// Test servers: approve every joiner as an editor without asking the host.
    pub auto_approve: bool,
    /// Secret a privileged connection presents to become the host or the replica (a test harness; it may then skip the
    /// handshake). Empty = only in-process connections can.
    pub local_token: String,
    /// Test servers only: network connections skip the handshake and frame encryption.
    pub plain: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            app: String::new(),
            schema: 1,
            max_clients: 20,
            log_keep: 10_000,
            auto_approve: false,
            local_token: String::new(),
            plain: false,
        }
    }
}

/// What the session wants done; `net.rs` carries these out in order.
pub enum Out {
    To(ConnId, Envelope),
    /// Close after sending what is queued for that connection.
    Close(ConnId, String),
    /// The handshake succeeded: seal and open every later frame of this connection with this session.
    Secure(ConnId, Box<crypto::Session>),
    /// Rotate this connection's frame keys (someone was removed): send the rekey frame before anything else.
    Rekey(ConnId),
}

impl std::fmt::Debug for Out {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Out::To(c, e) => write!(f, "To({c}, {})", e.t),
            Out::Close(c, why) => write!(f, "Close({c}, {why})"),
            Out::Secure(c, _) => write!(f, "Secure({c})"),
            Out::Rekey(c) => write!(f, "Rekey({c})"),
        }
    }
}

#[derive(Debug, Clone)]
struct Client {
    id: u32,
    name: String,
    device: String,
    via: String,
    kid: String,
    proof: Value,
    assertion: String,
    picture: Value,
    aud: String,
    ticket: Vec<u8>,
    role: Role,
    prefix: String,
    token: String,
    conn: Option<ConnId>,
    state: State,
    last_seen: Ms,
    gone_at: Ms,
    /// Highest edit counter already decided (committed or refused): a re-sent edit at or below it is ignored.
    last_n: u64,
    presence: Value,
    presence_dirty: bool,
    bucket: f64,
    bucket_at: Ms,
    backlog: VecDeque<Envelope>,
    warned: bool,
    hash: String,
    /// The last refusals sent (n → refuse body): sent again on rejoin.
    refusals: VecDeque<(u64, Value)>,
    /// An assistant client's person, 0 for people.
    of: u32,
}

/// A connection before and after it belongs to a client.
struct Conn {
    client: Option<u32>,
    trusted: bool,
    /// Handshake done (or not needed): envelopes other than `pake` are accepted.
    secure: bool,
    /// Frames on this connection are encrypted (so a removal rotates its keys).
    sealed: bool,
    hs: Option<Handshake>,
    via: String,
    kid: String,
    opened: Ms,
    aud: String,
    ticket: Vec<u8>,
    of: u32,
}

struct Handshake {
    kid: String,
    started: Ms,
    host: Option<crypto::HostHandshake>,
}

#[derive(Debug, Clone, PartialEq)]
enum KeyKind {
    Code(String),
    Link,
    Ticket,
    Assistant(u32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Handover {
    None,
    Waiting(u32),
    Done,
}

/// An invite secret the server accepts.
struct Key {
    kind: KeyKind,
    secret: Vec<u8>,
    context: String,
    one_time: bool,
    ttl: Ms,
    expires: Ms,
    wrong: u32,
}

/// One committed edit (the log).
#[derive(Debug, Clone)]
pub struct Commit {
    pub q: u64,
    pub c: u32,
    pub n: u64,
    pub label: String,
    pub ops: Value,
    /// The edit's touched keys without their versions (`{k, m, f?}`).
    pub touched: Value,
}

#[derive(Debug, Clone)]
struct Touch {
    k: String,
    f: Option<String>,
    r: Option<u64>,
    m: char,
    g: bool,
}

#[derive(Debug, Clone)]
struct Queued {
    c: u32,
    n: u64,
    label: String,
    ops: Value,
    touched: Vec<Touch>,
}

#[derive(Default)]
struct KeyState {
    rev: u64,
    by: u32,
    deleted: bool,
}

/// Counters for tests and the host's status line.
#[derive(Debug, Default, Clone)]
pub struct Stats {
    pub commits: u64,
    pub refused: u64,
    pub presence_out: u64,
    pub removed_flood: u64,
    pub wrong_tries: u64,
    pub rotations: u64,
}

/// The session.
pub struct Session {
    pub id: String,
    cfg: Config,
    clients: BTreeMap<u32, Client>,
    conns: HashMap<ConnId, Conn>,
    next_client: u32,
    seq: u64,
    log: VecDeque<Commit>,
    keys: HashMap<String, KeyState>,
    props: HashMap<String, (u64, u32)>,
    struct_rev: HashMap<String, u64>,
    holds: BTreeMap<String, (u32, Ms)>,
    locks: BTreeSet<String>,
    invites: BTreeMap<String, Key>,
    queue: VecDeque<Queued>,
    validating: Option<Queued>,
    replica: Option<u32>,
    saved_q: u64,
    saved_hash: String,
    snap_wait: VecDeque<u32>,
    snap_busy: Option<u32>,
    last_presence: Ms,
    last_ping: Ms,
    ended: bool,
    handover: Handover,
    carried: Vec<String>,
    now: Ms,
    pub stats: Stats,
}

// ---- value helpers (msg.rs of 0.5)

fn map(pairs: Vec<(&str, Value)>) -> Value {
    Value::map(pairs)
}
fn get<'a>(b: &'a Value, k: &str) -> Option<&'a Value> {
    b.get(k)
}
fn get_str<'a>(b: &'a Value, k: &str) -> Option<&'a str> {
    b.get(k).and_then(Value::as_str)
}
/// An unsigned integer field (integers only, as rmpv reads them).
fn get_u64(b: &Value, k: &str) -> Option<u64> {
    match b.get(k) {
        Some(Value::Int(i)) => u64::try_from(*i).ok(),
        _ => None,
    }
}
fn as_u64(v: &Value) -> Option<u64> {
    match v {
        Value::Int(i) => u64::try_from(*i).ok(),
        _ => None,
    }
}
fn get_arr<'a>(b: &'a Value, k: &str) -> &'a [Value] {
    b.get(k).map_or(&[], Value::as_arr)
}
fn as_bool(v: &Value) -> Option<bool> {
    if let Value::Bool(b) = v { Some(*b) } else { None }
}
fn get_bool(b: &Value, k: &str) -> Option<bool> {
    b.get(k).and_then(as_bool)
}
fn bin(v: &Value) -> Option<&[u8]> {
    if let Value::Bin(b) = v { Some(b) } else { None }
}
fn vz(n: usize) -> Value {
    Value::Int(i128::try_from(n).unwrap_or(0))
}
fn id32(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(0)
}
fn strs(v: &[String]) -> Value {
    Value::Arr(v.iter().map(|s| Value::from(s.as_str())).collect())
}

fn rand_hex(n: usize) -> Option<String> {
    let b = crypto::random(n)?;
    Some(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// Client-id prefixes for new object ids: short, never reused in a session.
fn prefix_for(id: u32) -> String {
    const A: &[u8] = b"abcdefghjkmnpqrstuvwxyz";
    let len = u32::try_from(A.len()).unwrap_or(23);
    let mut n = id;
    let mut s = String::new();
    loop {
        let d = usize::try_from(n.checked_rem(len).unwrap_or(0)).unwrap_or(0);
        s.insert(0, char::from(A.get(d).copied().unwrap_or(b'a')));
        n = n.checked_div(len).unwrap_or(0);
        if n == 0 {
            break;
        }
    }
    s
}

fn ticket_kid(c: u32) -> String {
    format!("h:{c}")
}

fn prop_key(k: &str, f: &str) -> String {
    format!("{k}\u{0}{f}")
}

fn holds_change(set: &[(String, u32)], clear: &[String]) -> Envelope {
    let s: Vec<(String, Value)> = set.iter().map(|(k, c)| (k.clone(), Value::from(*c))).collect();
    Envelope::new("holds").body(map(vec![("set", Value::Map(s)), ("clear", strs(clear))]))
}

fn commit_env(cm: &Commit) -> Envelope {
    let b = map(vec![
        ("c", cm.c.into()),
        ("n", cm.n.into()),
        ("label", cm.label.clone().into()),
        ("ops", cm.ops.clone()),
        ("touched", cm.touched.clone()),
    ]);
    let mut env = Envelope::new("commit").body(b);
    env.q = cm.q;
    env.c = u64::from(cm.c);
    env.n = cm.n;
    env
}

fn code_gone(kid: &str, reason: &str) -> Envelope {
    Envelope::new("code-gone").body(map(vec![("kid", kid.into()), ("reason", reason.into())]))
}

impl Session {
    /// A new session; None when the OS gives no randomness (no session id, no tokens).
    pub fn new(cfg: Config) -> Option<Self> {
        Some(Session {
            id: rand_hex(8)?,
            cfg,
            clients: BTreeMap::new(),
            conns: HashMap::new(),
            next_client: 1,
            seq: 0,
            log: VecDeque::new(),
            keys: HashMap::new(),
            props: HashMap::new(),
            struct_rev: HashMap::new(),
            holds: BTreeMap::new(),
            locks: BTreeSet::new(),
            invites: BTreeMap::new(),
            queue: VecDeque::new(),
            validating: None,
            replica: None,
            saved_q: 0,
            saved_hash: String::new(),
            snap_wait: VecDeque::new(),
            snap_busy: None,
            last_presence: 0,
            last_ping: 0,
            ended: false,
            handover: Handover::None,
            carried: vec![],
            now: 0,
            stats: Stats::default(),
        })
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }
    pub fn ended(&self) -> bool {
        self.ended
    }
    pub fn config(&self) -> &Config {
        &self.cfg
    }

    /// The host makes a short code from its own window (as an `invite` message does); for a host attached in-process.
    pub fn invite(&mut self, one_time: bool, ttl: Ms, now: Ms) -> Vec<Out> {
        let mut out = vec![];
        self.make_code(one_time, ttl.clamp(10_000, 24 * 3_600_000), None, now, &mut out);
        out
    }

    // ---------------------------------------------------------------------------------------------------------------
    // Handoff: the whole session state as one value, and a session made from it.

    /// Everything a new host needs to carry on (format `cadcraft-live-state`).
    pub fn export(&self) -> Value {
        self.state(None)
    }

    fn handover_state(&mut self, to: u32) -> Value {
        let sealed: BTreeSet<u32> =
            self.clients.values().filter(|c| c.conn.and_then(|k| self.conns.get(&k)).is_some_and(|x| x.sealed)).map(|c| c.id).collect();
        for c in self.clients.values_mut() {
            if c.id != to && c.role != Role::Replica && (c.ticket.is_empty() || !sealed.contains(&c.id)) {
                c.ticket = crypto::random(32).unwrap_or_default();
            }
        }
        self.state(Some(to))
    }

    fn state(&self, to: Option<u32>) -> Value {
        let s = |x: &str| Value::from(x);
        let mut undecided: HashMap<u32, u64> = HashMap::new();
        for e in self.queue.iter().chain(self.validating.iter()) {
            let x = undecided.entry(e.c).or_insert(e.n);
            *x = (*x).min(e.n);
        }
        let clients: Vec<Value> = self
            .clients
            .values()
            .filter(|c| c.state != State::Waiting && !c.token.is_empty() && c.role != Role::Replica)
            .map(|c| {
                let role = match to {
                    Some(t) if t == c.id => Role::Host,
                    Some(_) if c.role == Role::Host => Role::Editor,
                    _ => c.role,
                };
                let last_n = undecided.get(&c.id).map_or(c.last_n, |n| c.last_n.min(n.saturating_sub(1)));
                let mut pairs = vec![
                    ("c", c.id.into()),
                    ("name", s(&c.name)),
                    ("device", s(&c.device)),
                    ("role", s(role.name())),
                    ("prefix", s(&c.prefix)),
                    ("token", s(&c.token)),
                    ("last_n", last_n.into()),
                ];
                if !c.ticket.is_empty() {
                    pairs.push(("ticket", Value::Bin(c.ticket.clone())));
                }
                if c.of != 0 {
                    pairs.push(("of", c.of.into()));
                }
                map(pairs)
            })
            .collect();
        let log: Vec<Value> = self
            .log
            .iter()
            .map(|c| {
                map(vec![
                    ("q", c.q.into()),
                    ("c", c.c.into()),
                    ("n", c.n.into()),
                    ("label", s(&c.label)),
                    ("ops", c.ops.clone()),
                    ("touched", c.touched.clone()),
                ])
            })
            .collect();
        let mut keys: Vec<(&String, &KeyState)> = self.keys.iter().collect();
        keys.sort_by(|a, b| a.0.cmp(b.0));
        let keys: Vec<Value> = keys
            .into_iter()
            .map(|(k, v)| {
                Value::Arr(vec![s(k), v.rev.into(), v.by.into(), v.deleted.into(), self.struct_rev.get(k).copied().unwrap_or(0).into()])
            })
            .collect();
        let mut props: Vec<(&String, &(u64, u32))> = self.props.iter().collect();
        props.sort_by(|a, b| a.0.cmp(b.0));
        let props: Vec<Value> = props.into_iter().map(|(k, (q, by))| Value::Arr(vec![s(k), (*q).into(), (*by).into()])).collect();
        let invites: Vec<Value> = self
            .invites
            .values()
            .filter_map(|k| match &k.kind {
                KeyKind::Code(code) if k.expires > self.now => Some(map(vec![
                    ("code", s(code)),
                    ("secret", Value::Bin(k.secret.clone())),
                    ("one_time", k.one_time.into()),
                    ("left", k.expires.saturating_sub(self.now).into()),
                ])),
                _ => None,
            })
            .collect();
        map(vec![
            ("format", s("cadcraft-live-state")),
            ("v", 1u64.into()),
            ("session", s(&self.id)),
            ("app", s(&self.cfg.app)),
            ("schema", self.cfg.schema.into()),
            ("seq", self.seq.into()),
            ("next_client", self.next_client.into()),
            ("saved", map(vec![("q", self.saved_q.into()), ("hash", s(&self.saved_hash))])),
            ("locks", strs(&self.locks.iter().cloned().collect::<Vec<_>>())),
            ("clients", Value::Arr(clients)),
            ("log", Value::Arr(log)),
            ("keys", Value::Arr(keys)),
            ("props", Value::Arr(props)),
            ("invites", Value::Arr(invites)),
        ])
    }

    /// A session carrying on from `export()` (on the new host). Everyone starts as gone and comes back with `resume`.
    pub fn import(cfg: Config, st: &Value, now: Ms) -> Result<Session, String> {
        if get_str(st, "format") != Some("cadcraft-live-state") || get_u64(st, "v") != Some(1) {
            return Err("not a session state".into());
        }
        if get_str(st, "app").unwrap_or("") != cfg.app || get_u64(st, "schema") != Some(u64::from(cfg.schema)) {
            return Err("another app or schema".into());
        }
        let mut s = Session::new(cfg).ok_or("no randomness")?;
        s.id = get_str(st, "session").unwrap_or("").chars().take(64).collect();
        s.seq = get_u64(st, "seq").unwrap_or(0);
        s.next_client = id32(get_u64(st, "next_client").unwrap_or(1)).max(1);
        if let Some(sv) = get(st, "saved") {
            s.saved_q = get_u64(sv, "q").unwrap_or(0);
            s.saved_hash = get_str(sv, "hash").unwrap_or("").chars().take(200).collect();
        }
        s.locks = get_arr(st, "locks").iter().take(100_000).filter_map(|v| v.as_str().map(String::from)).collect();
        for c in get_arr(st, "clients").iter().take(10_000) {
            let id = id32(get_u64(c, "c").ok_or("client id")?);
            let role = get_str(c, "role").and_then(Role::parse).unwrap_or(Role::Editor);
            s.clients.insert(
                id,
                Client {
                    id,
                    name: get_str(c, "name").unwrap_or("").chars().take(80).collect(),
                    device: get_str(c, "device").unwrap_or("").chars().take(80).collect(),
                    via: "lan".into(),
                    kid: String::new(),
                    proof: Value::Nil,
                    assertion: String::new(),
                    picture: Value::Nil,
                    aud: String::new(),
                    ticket: match get(c, "ticket") {
                        Some(Value::Bin(t)) if t.len() == 32 => t.clone(),
                        _ => vec![],
                    },
                    role,
                    prefix: get_str(c, "prefix").unwrap_or("").chars().take(16).collect(),
                    token: get_str(c, "token").unwrap_or("").chars().take(64).collect(),
                    conn: None,
                    state: State::Gone,
                    last_seen: now,
                    gone_at: now,
                    last_n: get_u64(c, "last_n").unwrap_or(0),
                    presence: Value::Nil,
                    presence_dirty: false,
                    bucket: RATE_PER_SEC,
                    bucket_at: now,
                    backlog: VecDeque::new(),
                    warned: false,
                    hash: String::new(),
                    refusals: VecDeque::new(),
                    of: id32(get_u64(c, "of").unwrap_or(0)),
                },
            );
        }
        for e in get_arr(st, "log").iter().take(s.cfg.log_keep.max(1)) {
            s.log.push_back(Commit {
                q: get_u64(e, "q").ok_or("log seq")?,
                c: id32(get_u64(e, "c").unwrap_or(0)),
                n: get_u64(e, "n").unwrap_or(0),
                label: get_str(e, "label").unwrap_or("").chars().take(200).collect(),
                ops: get(e, "ops").cloned().unwrap_or(Value::Nil),
                touched: get(e, "touched").cloned().unwrap_or(Value::Arr(vec![])),
            });
        }
        for k in get_arr(st, "keys") {
            let a = k.as_arr();
            let name = a.first().and_then(Value::as_str).ok_or("key name")?.to_string();
            let sr = a.get(4).and_then(as_u64).unwrap_or(0);
            if sr > 0 {
                s.struct_rev.insert(name.clone(), sr);
            }
            s.keys.insert(
                name,
                KeyState {
                    rev: a.get(1).and_then(as_u64).unwrap_or(0),
                    by: id32(a.get(2).and_then(as_u64).unwrap_or(0)),
                    deleted: a.get(3).and_then(as_bool).unwrap_or(false),
                },
            );
        }
        for p in get_arr(st, "props") {
            let a = p.as_arr();
            let k = a.first().and_then(Value::as_str).ok_or("prop key")?.to_string();
            s.props.insert(k, (a.get(1).and_then(as_u64).unwrap_or(0), id32(a.get(2).and_then(as_u64).unwrap_or(0))));
        }
        let context = format!("rejoin:{}", s.id);
        let tickets: Vec<(u32, Vec<u8>)> = s.clients.values().filter(|c| !c.ticket.is_empty()).map(|c| (c.id, c.ticket.clone())).collect();
        for (id, t) in tickets {
            s.invites.insert(
                ticket_kid(id),
                Key {
                    kind: KeyKind::Ticket,
                    secret: t,
                    context: context.clone(),
                    one_time: false,
                    ttl: TICKET_TTL,
                    expires: TICKET_TTL,
                    wrong: 0,
                },
            );
        }
        for i in get_arr(st, "invites").iter().take(1000) {
            let (Some(code), Some(Value::Bin(secret))) = (get_str(i, "code"), get(i, "secret")) else { continue };
            let Some((tag, parsed)) = crypto::parse_code(code) else { continue };
            if &parsed != secret || s.invites.contains_key(&tag) {
                continue;
            }
            let left = get_u64(i, "left").unwrap_or(0).min(24 * 3_600_000);
            if left == 0 {
                continue;
            }
            let one_time = get_bool(i, "one_time").unwrap_or(false);
            s.invites.insert(
                tag.clone(),
                Key {
                    kind: KeyKind::Code(code.to_string()),
                    secret: parsed,
                    context: format!("lan:{tag}"),
                    one_time,
                    ttl: left,
                    expires: now.saturating_add(left),
                    wrong: 0,
                },
            );
            s.carried.push(tag);
        }
        Ok(s)
    }

    // ---------------------------------------------------------------------------------------------------------------
    // Connections

    /// A transport connection opened. `trusted` = in-process (the host's window, the replica); `via` = how it came
    /// (`local`, `lan`, `relay`), decided by the transport.
    pub fn open(&mut self, conn: ConnId, trusted: bool, via: &str, now: Ms) {
        let secure = trusted || self.cfg.plain;
        self.conns.insert(
            conn,
            Conn {
                client: None,
                trusted,
                secure,
                sealed: false,
                hs: None,
                via: via.into(),
                kid: String::new(),
                opened: now,
                aud: String::new(),
                ticket: vec![],
                of: 0,
            },
        );
    }

    /// The transport closed (or a frame failed to open: net.rs closes and calls this).
    pub fn closed(&mut self, conn: ConnId, now: Ms) -> Vec<Out> {
        let mut out = vec![];
        if let Some(cx) = self.conns.remove(&conn) {
            if let Some(hs) = cx.hs {
                // Quit after message 2 without proving the code: a wrong try (the guesser rule).
                self.wrong_try(&hs.kid, now, &mut out);
            }
            if let Some(c) = cx.client
                && self.clients.get(&c).map(|x| x.conn) == Some(Some(conn))
            {
                self.drop_client(c, now, false, &mut out);
            }
        }
        out
    }

    fn drop_client(&mut self, c: u32, now: Ms, removed: bool, out: &mut Vec<Out>) {
        let Some(cl) = self.clients.get_mut(&c) else { return };
        if cl.state == State::Gone {
            return;
        }
        let was_waiting = cl.state == State::Waiting;
        let role = cl.role;
        cl.state = State::Gone;
        cl.gone_at = now;
        if self.handover == Handover::Waiting(c) {
            self.handover = Handover::None;
            let b = map(vec![("to", c.into()), ("reason", "gone".into())]);
            self.to_role(Role::Host, Envelope::new("handover-off").body(b), out);
        }
        if removed {
            if let Some(cl) = self.clients.get_mut(&c) {
                cl.token.clear();
            }
            self.invites.remove(&ticket_kid(c));
        }
        if self.clients.get(&c).is_some_and(|cl| cl.token.is_empty()) {
            self.remove_assistants(c, now, out);
        }
        let Some(cl) = self.clients.get_mut(&c) else { return };
        if let Some(conn) = cl.conn.take() {
            self.conns.remove(&conn);
            out.push(Out::Close(conn, if removed { "removed".into() } else { "gone".into() }));
        }
        if was_waiting {
            self.clients.remove(&c);
            return;
        }
        if self.replica == Some(c) {
            self.replica = None;
            if let Some(e) = self.validating.take() {
                self.queue.push_front(e);
            }
        }
        if self.snap_busy == Some(c) {
            self.snap_busy = None;
        }
        self.snap_wait.retain(|x| *x != c);
        let released: Vec<String> = self.holds.iter().filter(|(_, (h, _))| *h == c).map(|(k, _)| k.clone()).collect();
        for k in &released {
            self.holds.remove(k);
        }
        if !released.is_empty() {
            self.broadcast(&holds_change(&[], &released), None, out);
        }
        if role != Role::Replica {
            let b = map(vec![("c", c.into()), ("state", if removed { "removed" } else { "gone" }.into())]);
            self.broadcast(&Envelope::new("peer").body(b), Some(c), out);
        }
        if role == Role::Host && !removed {
            self.end("host-left", out);
        }
        self.pump_snapshots(out);
    }

    // ---------------------------------------------------------------------------------------------------------------
    // Frames

    /// One envelope from a connection (opened by net.rs when the connection is sealed).
    pub fn frame(&mut self, conn: ConnId, env: Envelope, now: Ms) -> Vec<Out> {
        self.now = now;
        let mut out = vec![];
        if self.ended {
            out.push(Out::Close(conn, "ended".into()));
            return out;
        }
        let Some(cx) = self.conns.get(&conn) else { return out };
        if !cx.secure {
            let tok = !self.cfg.local_token.is_empty() && get_str(&env.b, "token") == Some(self.cfg.local_token.as_str());
            if env.t == "hello" && tok && cx.client.is_none() {
                if let Some(cx) = self.conns.get_mut(&conn) {
                    cx.secure = true;
                }
                self.on_hello(conn, env, now, &mut out);
                self.pump(&mut out);
                return out;
            }
            if env.t == "pake" {
                self.on_pake(conn, &env, now, &mut out);
            } else {
                self.conns.remove(&conn);
                out.push(Out::Close(conn, "handshake expected".into()));
            }
            return out;
        }
        match cx.client {
            None => self.on_hello(conn, env, now, &mut out),
            Some(c) => {
                if let Some(cl) = self.clients.get_mut(&c) {
                    cl.last_seen = now;
                }
                self.on_message(c, conn, env, now, &mut out);
            }
        }
        self.pump(&mut out);
        out
    }

    // ---------------------------------------------------------------------------------------------------------------
    // Handshake: message 1 {kid, m} → 2 {m} → 3 {m} → 4 {m}, then sealed frames.

    fn on_pake(&mut self, conn: ConnId, env: &Envelope, now: Ms, out: &mut Vec<Out>) {
        let m = get(&env.b, "m").and_then(bin).map(<[u8]>::to_vec).unwrap_or_default();
        let has_hs = self.conns.get(&conn).is_some_and(|cx| cx.hs.is_some());
        if !has_hs {
            let kid = get_str(&env.b, "kid").unwrap_or("").chars().take(128).collect::<String>();
            let Some(key) = self.invites.get(&kid).filter(|k| k.expires > now) else {
                // Unknown or expired secret: nothing was tried, so not counted.
                self.conns.remove(&conn);
                out.push(Out::Close(conn, "unknown code".into()));
                return;
            };
            let mut host = crypto::HostHandshake::new(&key.secret, &key.context);
            match host.receive(&m) {
                Ok(reply) => {
                    if let Some(cx) = self.conns.get_mut(&conn) {
                        cx.kid = kid.clone();
                        cx.hs = Some(Handshake { kid, started: now, host: Some(host) });
                    }
                    out.push(Out::To(conn, Envelope::new("pake").body(map(vec![("m", Value::Bin(reply))]))));
                }
                Err(_) => {
                    self.conns.remove(&conn);
                    out.push(Out::Close(conn, "bad handshake".into()));
                }
            }
            return;
        }
        let Some(cx) = self.conns.get_mut(&conn) else { return };
        let Some(hs) = cx.hs.as_mut() else { return };
        let kid = hs.kid.clone();
        let Some(host) = hs.host.take() else { return };
        match host.finish(&m) {
            Ok((confirm, session)) => {
                cx.hs = None;
                cx.secure = true;
                cx.sealed = true;
                cx.aud = crypto::b64url(&session.exporter("assertion"));
                cx.ticket = session.exporter("ticket").to_vec();
                out.push(Out::To(conn, Envelope::new("pake").body(map(vec![("m", Value::Bin(confirm))]))));
                out.push(Out::Secure(conn, Box::new(session)));
                let mut of = 0;
                let mut used = false;
                if let Some(key) = self.invites.get_mut(&kid) {
                    key.wrong = 0;
                    if let KeyKind::Assistant(p) = &key.kind {
                        of = *p;
                    }
                    used = key.one_time;
                }
                if used {
                    self.invites.remove(&kid);
                    if of == 0 {
                        self.to_role(Role::Host, code_gone(&kid, "used"), out);
                    }
                }
                if of != 0
                    && let Some(cx) = self.conns.get_mut(&conn)
                {
                    cx.of = of;
                }
            }
            Err(_) => {
                cx.hs = None;
                self.conns.remove(&conn);
                out.push(Out::Close(conn, "wrong code".into()));
                self.wrong_try(&kid, now, out);
            }
        }
    }

    /// Counts a wrong try against a secret; three in a row replace it.
    fn wrong_try(&mut self, kid: &str, now: Ms, out: &mut Vec<Out>) {
        self.stats.wrong_tries = self.stats.wrong_tries.saturating_add(1);
        let Some(key) = self.invites.get_mut(kid) else { return };
        key.wrong = key.wrong.saturating_add(1);
        if key.wrong < WRONG_TRIES {
            return;
        }
        self.stats.rotations = self.stats.rotations.saturating_add(1);
        let Some(key) = self.invites.remove(kid) else { return };
        if matches!(key.kind, KeyKind::Ticket | KeyKind::Assistant(_)) {
            return;
        }
        self.to_role(Role::Host, code_gone(kid, "wrong-tries"), out);
        if let KeyKind::Code(_) = key.kind {
            self.make_code(key.one_time, key.ttl, Some(kid), now, out);
        }
    }

    /// A new short code, told to the host.
    fn make_code(&mut self, one_time: bool, ttl: Ms, replaces: Option<&str>, now: Ms, out: &mut Vec<Out>) {
        let mut made = None;
        for _ in 0..64 {
            let Some(code) = crypto::new_code() else { break };
            let Some((tag, secret)) = crypto::parse_code(&code) else { continue };
            if !self.invites.contains_key(&tag) {
                made = Some((code, tag, secret));
                break;
            }
        }
        let Some((code, tag, secret)) = made else { return };
        self.invites.insert(
            tag.clone(),
            Key {
                kind: KeyKind::Code(code.clone()),
                secret,
                context: format!("lan:{tag}"),
                one_time,
                ttl,
                expires: now.saturating_add(ttl),
                wrong: 0,
            },
        );
        let mut pairs = vec![
            ("code", Value::from(code)),
            ("tag", Value::from(tag)),
            ("expires", Value::from(ttl)),
            ("one_time", Value::from(one_time)),
        ];
        if let Some(old) = replaces {
            pairs.push(("reason", "wrong-tries".into()));
            pairs.push(("replaces", old.into()));
        }
        self.to_role(Role::Host, Envelope::new("code").body(map(pairs)), out);
    }

    /// Something for the host's window from outside the session.
    pub fn notice(&mut self, env: Envelope) -> Vec<Out> {
        let mut out = vec![];
        self.to_role(Role::Host, env, &mut out);
        out
    }

    // ---------------------------------------------------------------------------------------------------------------
    // hello

    fn deny(&mut self, conn: ConnId, reason: &str, out: &mut Vec<Out>) {
        self.conns.remove(&conn);
        out.push(Out::To(conn, Envelope::new("deny").body(map(vec![("reason", reason.into())]))));
        out.push(Out::Close(conn, reason.into()));
    }

    fn on_hello(&mut self, conn: ConnId, env: Envelope, now: Ms, out: &mut Vec<Out>) {
        if env.t != "hello" {
            self.conns.remove(&conn);
            out.push(Out::Close(conn, "hello expected".into()));
            return;
        }
        let b = &env.b;
        if get_u64(b, "v").unwrap_or(0) != PROTOCOL {
            return self.deny(conn, "protocol", out);
        }
        let app = get_str(b, "app").unwrap_or("");
        if !self.cfg.app.is_empty() && app != self.cfg.app {
            return self.deny(conn, "app", out);
        }
        if get_u64(b, "schema").unwrap_or(0) != u64::from(self.cfg.schema) {
            return self.deny(conn, "schema", out);
        }
        let Some(trusted) = self.conns.get(&conn).map(|x| x.trusted) else { return };
        let tok_ok = !self.cfg.local_token.is_empty() && get_str(b, "token") == Some(self.cfg.local_token.as_str());
        // A privileged connection claims host / replica: in-process, or a test server's local token.
        let want = get_str(b, "role").and_then(Role::parse);
        if let Some(role @ (Role::Host | Role::Replica)) = want {
            if !(trusted || tok_ok) {
                return self.deny(conn, "role", out);
            }
            if role == Role::Host && self.clients.values().any(|c| c.role == Role::Host && c.state != State::Gone) {
                return self.deny(conn, "host-taken", out);
            }
            let Some(c) = self.new_client(conn, b, role, now) else { return self.deny(conn, "full", out) };
            if role == Role::Replica {
                self.replica = Some(c);
            }
            self.activate(c, out);
            return;
        }
        // Rejoin: client id + resume token, no new approval.
        if let Some(r) = get(b, "resume") {
            self.on_resume(conn, b, r, now, out);
            return;
        }
        // An assistant: its person's one-time key, through the handshake (`Conn::of`) or in `hello.assistant`.
        let mut of = self.conns.get(&conn).map_or(0, |x| x.of);
        if of == 0
            && let Some(a) = get(b, "assistant")
        {
            let open = trusted || self.cfg.plain || tok_ok;
            let kid = get_str(a, "kid").unwrap_or("").to_string();
            let sec = get(a, "secret").and_then(bin).map(<[u8]>::to_vec).unwrap_or_default();
            let ok = open
                && matches!(self.invites.get(&kid), Some(k) if matches!(k.kind, KeyKind::Assistant(_)) && k.expires > now && crypto::same(&k.secret, &sec));
            if !ok {
                return self.deny(conn, "role", out);
            }
            if let Some(Key { kind: KeyKind::Assistant(p), .. }) = self.invites.remove(&kid) {
                of = p;
            }
        }
        if of != 0 {
            let person_ok = self.clients.get(&of).is_some_and(|p| p.state == State::Active && matches!(p.role, Role::Host | Role::Editor));
            if !person_ok {
                return self.deny(conn, "role", out);
            }
        }
        // max_clients people besides the host
        let people = self.clients.values().filter(|c| c.state != State::Gone && c.role != Role::Replica).count();
        if people > self.cfg.max_clients {
            return self.deny(conn, "full", out);
        }
        let Some(c) = self.new_client(conn, b, if of != 0 { Role::Assistant } else { Role::Editor }, now) else {
            return self.deny(conn, "full", out);
        };
        if let Some(cl) = self.clients.get_mut(&c) {
            cl.of = of;
        }
        let host_own = of != 0 && self.clients.get(&of).is_some_and(|p| p.role == Role::Host);
        if self.cfg.auto_approve || host_own {
            self.activate(c, out);
            return;
        }
        out.push(Out::To(conn, Envelope::new("waiting")));
        let Some(cl) = self.clients.get(&c) else { return };
        let mut jb = vec![
            ("c", c.into()),
            ("name", cl.name.clone().into()),
            ("device", cl.device.clone().into()),
            ("via", cl.via.clone().into()),
            ("kid", cl.kid.clone().into()),
            ("proof", cl.proof.clone()),
            ("assertion", cl.assertion.clone().into()),
            ("picture", cl.picture.clone()),
        ];
        if !cl.aud.is_empty() {
            jb.push(("aud", cl.aud.clone().into()));
        }
        if cl.of != 0 {
            let pn = self.clients.get(&cl.of).map(|p| p.name.clone()).unwrap_or_default();
            jb.push(("assistant", map(vec![("of", cl.of.into()), ("of_name", pn.into())])));
        }
        let jb = map(jb);
        self.to_role(Role::Host, Envelope::new("join").body(jb), out);
    }

    fn on_resume(&mut self, conn: ConnId, b: &Value, r: &Value, now: Ms, out: &mut Vec<Out>) {
        let rc = id32(get_u64(r, "c").unwrap_or(0));
        let tok = get_str(r, "token").unwrap_or("");
        let ok = self
            .clients
            .get(&rc)
            .map(|cl| !cl.token.is_empty() && crypto::same(cl.token.as_bytes(), tok.as_bytes()) && cl.state != State::Waiting);
        if ok != Some(true) {
            return self.deny(conn, "resume", out);
        }
        if let Some(old) = self.clients.get(&rc).and_then(|cl| cl.conn) {
            self.conns.remove(&old);
            out.push(Out::Close(old, "replaced".into()));
        }
        let from = get_u64(r, "q").unwrap_or(0);
        let (via, kid, aud, ticket) = match self.conns.get(&conn) {
            Some(cx) => (cx.via.clone(), cx.kid.clone(), cx.aud.clone(), cx.ticket.clone()),
            None => return,
        };
        let Some(cl) = self.clients.get_mut(&rc) else { return };
        let was_gone = cl.state == State::Gone;
        cl.conn = Some(conn);
        cl.state = State::Active;
        cl.last_seen = now;
        cl.via = via;
        if !kid.is_empty() && !kid.starts_with("h:") {
            cl.kid = kid;
        }
        cl.aud = aud;
        if cl.ticket.is_empty() && !ticket.is_empty() {
            cl.ticket = ticket;
        }
        cl.hash = get_str(b, "hash").unwrap_or("").chars().take(200).collect();
        if let Some(cx) = self.conns.get_mut(&conn) {
            cx.client = Some(rc);
        }
        let mut taken = vec![];
        for k in get_arr(r, "holds").iter().filter_map(Value::as_str).take(10_000) {
            if !self.holds.contains_key(k) {
                self.holds.insert(k.to_string(), (rc, now.saturating_add(HOLD_TTL)));
                taken.push((k.to_string(), rc));
            }
        }
        self.welcome(rc, Some(from), out);
        self.rejoin_key(rc);
        if self.clients.get(&rc).is_some_and(|c| c.role == Role::Host) {
            self.tell_carried(out);
        }
        let oldest = get_u64(r, "n").unwrap_or(u64::MAX);
        let again: Vec<Envelope> = self
            .clients
            .get(&rc)
            .map(|cl| {
                cl.refusals
                    .iter()
                    .filter(|(n, _)| *n >= oldest)
                    .map(|(n, b)| {
                        let mut e = Envelope::new("refuse").body(b.clone());
                        e.n = *n;
                        e
                    })
                    .collect()
            })
            .unwrap_or_default();
        for e in again {
            out.push(Out::To(conn, e));
        }
        if !taken.is_empty() {
            self.broadcast(&holds_change(&taken, &[]), None, out);
        }
        if was_gone {
            let pb = map(vec![("c", rc.into()), ("state", "back".into())]);
            self.broadcast(&Envelope::new("peer").body(pb), Some(rc), out);
        }
    }

    fn new_client(&mut self, conn: ConnId, b: &Value, role: Role, now: Ms) -> Option<u32> {
        let token = rand_hex(16)?;
        let id = self.next_client;
        self.next_client = self.next_client.checked_add(1)?;
        let cx = self.conns.get_mut(&conn)?;
        cx.client = Some(id);
        let cl = Client {
            id,
            name: get_str(b, "name").unwrap_or("").chars().take(80).collect(),
            device: get_str(b, "device").unwrap_or("").chars().take(80).collect(),
            via: cx.via.clone(),
            kid: cx.kid.clone(),
            proof: get(b, "proof").cloned().filter(|p| crate::msgpack::pack(p).is_ok_and(|x| x.len() <= 4096)).unwrap_or(Value::Nil),
            assertion: get_str(b, "assertion").filter(|a| a.len() <= 2048).unwrap_or("").to_string(),
            picture: match get(b, "picture") {
                Some(Value::Bin(p)) if p.len() <= 16 * 1024 => Value::Bin(p.clone()),
                _ => Value::Nil,
            },
            aud: cx.aud.clone(),
            ticket: cx.ticket.clone(),
            role,
            prefix: prefix_for(id),
            token,
            conn: Some(conn),
            state: State::Waiting,
            last_seen: now,
            gone_at: 0,
            last_n: 0,
            presence: Value::Nil,
            presence_dirty: false,
            bucket: RATE_PER_SEC,
            bucket_at: now,
            backlog: VecDeque::new(),
            warned: false,
            hash: get_str(b, "hash").unwrap_or("").chars().take(200).collect(),
            refusals: VecDeque::new(),
            of: 0,
        };
        self.clients.insert(id, cl);
        Some(id)
    }

    /// A person in over a sealed connection gets a rejoin key: their connection's `exporter("ticket")` under `h:<c>`.
    fn rejoin_key(&mut self, c: u32) {
        let Some(cl) = self.clients.get(&c) else { return };
        let sealed = cl.conn.and_then(|k| self.conns.get(&k)).is_some_and(|x| x.sealed);
        let kid = ticket_kid(c);
        if !sealed || cl.ticket.is_empty() || self.invites.contains_key(&kid) {
            return;
        }
        let key = Key {
            kind: KeyKind::Ticket,
            secret: cl.ticket.clone(),
            context: format!("rejoin:{}", self.id),
            one_time: false,
            ttl: TICKET_TTL,
            expires: TICKET_TTL,
            wrong: 0,
        };
        self.invites.insert(kid, key);
    }

    /// The new host is in after a handoff: the codes that came with the session, as `code {…, carried: true}`.
    fn tell_carried(&mut self, out: &mut Vec<Out>) {
        for tag in std::mem::take(&mut self.carried) {
            let Some(k) = self.invites.get(&tag) else { continue };
            let KeyKind::Code(code) = &k.kind else { continue };
            let b = map(vec![
                ("code", code.as_str().into()),
                ("tag", tag.as_str().into()),
                ("expires", k.expires.saturating_sub(self.now).into()),
                ("one_time", k.one_time.into()),
                ("carried", true.into()),
            ]);
            self.to_role(Role::Host, Envelope::new("code").body(b), out);
        }
    }

    fn activate(&mut self, c: u32, out: &mut Vec<Out>) {
        let Some(cl) = self.clients.get_mut(&c) else { return };
        cl.state = State::Active;
        let role = cl.role;
        self.welcome(c, None, out);
        self.rejoin_key(c);
        if role == Role::Host {
            self.tell_carried(out);
        }
        if role != Role::Replica {
            let Some(cl) = self.clients.get(&c) else { return };
            let mut pb = vec![
                ("c", c.into()),
                ("state", "in".into()),
                ("name", cl.name.clone().into()),
                ("role", role.name().into()),
                ("via", cl.via.clone().into()),
            ];
            if cl.of != 0 {
                pb.push(("of", cl.of.into()));
            }
            self.broadcast(&Envelope::new("peer").body(map(pb)), Some(c), out);
        }
    }

    /// `welcome` + catch-up: the commits since `from` (rejoin) or since the last save (same file), else a snapshot.
    fn welcome(&mut self, c: u32, from: Option<u64>, out: &mut Vec<Out>) {
        let Some(cl) = self.clients.get(&c) else { return };
        let Some(conn) = cl.conn else { return };
        let reach = self.log.front().map_or(self.seq, |x| x.q.saturating_sub(1));
        let from = match from {
            Some(q) if q <= self.seq && q >= reach => Some(q),
            Some(_) => None,
            None if !cl.hash.is_empty() && cl.hash == self.saved_hash && self.saved_q >= reach => Some(self.saved_q),
            None => None,
        };
        let peers: Vec<Value> = self
            .clients
            .values()
            .filter(|p| p.id != c && p.state == State::Active && p.role != Role::Replica)
            .map(|p| {
                let mut v = vec![
                    ("c", p.id.into()),
                    ("name", p.name.clone().into()),
                    ("role", p.role.name().into()),
                    ("via", p.via.clone().into()),
                ];
                if p.of != 0 {
                    v.push(("of", p.of.into()));
                }
                map(v)
            })
            .collect();
        let holds: Vec<(String, Value)> = self.holds.iter().map(|(k, (h, _))| (k.clone(), Value::from(*h))).collect();
        let locks: Vec<Value> = self.locks.iter().map(|k| Value::from(k.as_str())).collect();
        let assistant = cl.of != 0;
        let catchup = match (cl.role, from) {
            (Role::Replica, _) => "none",
            (_, Some(_)) => "log",
            (_, None) if assistant => "none",
            (_, None) => "snapshot",
        };
        let b = vec![
            ("c", c.into()),
            ("prefix", cl.prefix.clone().into()),
            ("role", cl.role.name().into()),
            ("token", cl.token.clone().into()),
            ("session", self.id.clone().into()),
            ("app", self.cfg.app.clone().into()),
            ("schema", self.cfg.schema.into()),
            ("peers", Value::Arr(peers)),
            ("holds", Value::Map(holds)),
            ("locks", Value::Arr(locks)),
            ("saved", map(vec![("q", self.saved_q.into()), ("hash", self.saved_hash.clone().into())])),
            ("catchup", catchup.into()),
            ("last_n", cl.last_n.into()),
        ];
        let mut env = Envelope::new("welcome").body(map(b));
        env.s = self.id.clone();
        env.q = self.seq;
        out.push(Out::To(conn, env));
        match (catchup, from) {
            ("log", Some(from)) => {
                for cm in self.log.iter().filter(|x| x.q > from) {
                    out.push(Out::To(conn, commit_env(cm)));
                }
            }
            ("snapshot", _) => {
                self.snap_wait.push_back(c);
                self.pump_snapshots(out);
            }
            _ => {}
        }
    }

    /// One snapshot at a time: ask the replica, or the host window when there is no replica.
    fn pump_snapshots(&mut self, out: &mut Vec<Out>) {
        if self.snap_busy.is_some() {
            return;
        }
        while let Some(c) = self.snap_wait.pop_front() {
            if !self.clients.get(&c).is_some_and(|x| x.state == State::Active) {
                continue;
            }
            let src = self.replica.or_else(|| self.role_client(Role::Host)).filter(|s| *s != c);
            let Some(src) = src else {
                // Nobody can make one (a test server without host or replica): an empty snapshot at seq 0, then the log.
                let e = Envelope::new("snap").body(map(vec![("i", 0u64.into()), ("of", 0u64.into())]));
                self.send(c, e, out);
                let reach_ok = self.log.front().is_none_or(|x| x.q <= 1);
                if reach_ok {
                    let all: Vec<Envelope> = self.log.iter().map(commit_env).collect();
                    for e in all {
                        self.send(c, e, out);
                    }
                }
                continue;
            };
            self.snap_busy = Some(c);
            self.send(src, Envelope::new("snapreq").body(map(vec![("for", c.into())])), out);
            return;
        }
    }

    // ---------------------------------------------------------------------------------------------------------------
    // Messages from a member

    fn on_message(&mut self, c: u32, conn: ConnId, env: Envelope, now: Ms, out: &mut Vec<Out>) {
        let Some((role, state)) = self.clients.get(&c).map(|cl| (cl.role, cl.state)) else { return };
        if state == State::Waiting {
            // Nothing reaches a joiner before approval, and nothing from it counts except bye.
            if env.t == "bye" {
                self.conns.remove(&conn);
                self.clients.remove(&c);
                out.push(Out::Close(conn, "bye".into()));
            }
            return;
        }
        let keys = |b: &Value, max: usize| -> Vec<String> {
            get_arr(b, "keys").iter().take(max).filter_map(|v| v.as_str().map(String::from)).collect()
        };
        match env.t.as_str() {
            "ping" => {
                let mut e = Envelope::new("pong");
                e.n = env.n;
                out.push(Out::To(conn, e));
            }
            "pong" => {}
            "bye" => {
                if let Some(cl) = self.clients.get_mut(&c) {
                    cl.token.clear();
                }
                self.drop_client(c, now, false, out);
            }
            "edit" => self.on_edit(c, conn, env, now, out),
            "assistant" => self.on_assistant(c, conn, now, out),
            "hold" => {
                let mut set = vec![];
                for k in keys(&env.b, 10_000) {
                    match self.holds.get(&k) {
                        Some((h, _)) if *h != c => {}
                        prev => {
                            let fresh = prev.is_none();
                            self.holds.insert(k.clone(), (c, now.saturating_add(HOLD_TTL)));
                            if fresh {
                                set.push((k, c));
                            }
                        }
                    }
                }
                if !set.is_empty() {
                    self.broadcast(&holds_change(&set, &[]), None, out);
                }
            }
            "release" => {
                let all = get_bool(&env.b, "all") == Some(true);
                let ks: Vec<String> = if all {
                    self.holds.iter().filter(|(_, (h, _))| *h == c).map(|(k, _)| k.clone()).collect()
                } else {
                    keys(&env.b, 100_000)
                };
                let mut gone = vec![];
                for k in ks {
                    if self.holds.get(&k).map(|(h, _)| *h) == Some(c) {
                        self.holds.remove(&k);
                        gone.push(k);
                    }
                }
                if !gone.is_empty() {
                    self.broadcast(&holds_change(&[], &gone), None, out);
                }
            }
            "presence" => {
                if let Some(cl) = self.clients.get_mut(&c) {
                    // a peer's presence is small (cursor, view, selection, command): a huge one is dropped
                    if crate::msgpack::pack(&env.b).is_ok_and(|b| b.len() <= 64 * 1024) {
                        cl.presence = env.b;
                        cl.presence_dirty = true;
                    }
                }
            }
            // ---- host only
            "approve" | "deny" | "remove" | "role" if role == Role::Host => self.host_people(&env, now, out),
            "lock" | "unlock" if role == Role::Host => {
                let (mut add, mut del) = (vec![], vec![]);
                for k in keys(&env.b, 100_000) {
                    if env.t == "lock" {
                        if self.locks.insert(k.clone()) {
                            add.push(k);
                        }
                    } else if self.locks.remove(&k) {
                        del.push(k);
                    }
                }
                if !add.is_empty() || !del.is_empty() {
                    let b = map(vec![("add", strs(&add)), ("del", strs(&del))]);
                    self.broadcast(&Envelope::new("locks").body(b), None, out);
                }
            }
            "invite" if role == Role::Host => {
                let one_time = get_bool(&env.b, "one_time").unwrap_or(false);
                let ttl = get_u64(&env.b, "ttl").unwrap_or(CODE_TTL).clamp(10_000, 24 * 3_600_000);
                self.make_code(one_time, ttl, None, now, out);
            }
            "key" if role == Role::Host => {
                let kid = get_str(&env.b, "kid").unwrap_or("").to_string();
                let secret = get(&env.b, "secret").and_then(bin).map(<[u8]>::to_vec).unwrap_or_default();
                let context = get_str(&env.b, "context").unwrap_or("").to_string();
                if kid.is_empty()
                    || kid.len() > 64
                    || secret.len() < 16
                    || secret.len() > 64
                    || !context.starts_with("relay:")
                    || context.len() > 128
                {
                    return;
                }
                let ttl = get_u64(&env.b, "ttl").unwrap_or(8 * 3_600_000).min(30 * 24 * 3_600_000);
                let one_time = get_bool(&env.b, "one_time").unwrap_or(false);
                self.invites
                    .insert(kid, Key { kind: KeyKind::Link, secret, context, one_time, ttl, expires: now.saturating_add(ttl), wrong: 0 });
            }
            "uninvite" if role == Role::Host => {
                let kids: Vec<String> = match get_str(&env.b, "kid") {
                    Some(k) => vec![k.to_string()],
                    None => self.invites.iter().filter(|(_, k)| k.kind != KeyKind::Ticket).map(|(k, _)| k.clone()).collect(),
                };
                for k in kids {
                    if self.invites.remove(&k).is_some() {
                        self.to_role(Role::Host, code_gone(&k, "stopped"), out);
                    }
                }
            }
            "save" if role == Role::Host => {
                self.saved_q = get_u64(&env.b, "q").unwrap_or(self.seq).min(self.seq);
                self.saved_hash = get_str(&env.b, "hash").unwrap_or("").chars().take(200).collect();
                let at =
                    get(&env.b, "at").cloned().filter(|v| matches!(v, Value::Int(_) | Value::F64(_) | Value::Str(_))).unwrap_or(Value::Nil);
                let b = map(vec![("q", self.saved_q.into()), ("hash", self.saved_hash.clone().into()), ("at", at)]);
                self.broadcast(&Envelope::new("saved").body(b), None, out);
            }
            "end" if role == Role::Host => self.end("host-ended", out),
            "handover" if role == Role::Host => self.on_handover(c, &env, out),
            "moved" => self.on_moved(c, &env, out),
            "valid" if role == Role::Replica => self.on_valid(&env, out),
            "snap" if role == Role::Replica || role == Role::Host => self.on_snap(&env, out),
            _ => {}
        }
    }

    /// `assistant`: a person asks for a one-time key for the assistant connected to their window.
    fn on_assistant(&mut self, c: u32, conn: ConnId, now: Ms, out: &mut Vec<Out>) {
        let reply = |b: Value| Envelope::new("assistant-key").body(b);
        let Some(cl) = self.clients.get(&c) else { return };
        if !matches!(cl.role, Role::Host | Role::Editor) || cl.of != 0 {
            out.push(Out::To(conn, reply(map(vec![("reason", "role".into())]))));
            return;
        }
        let used = self
            .clients
            .values()
            .filter(|x| x.of == c && x.state != State::Gone)
            .count()
            .saturating_add(self.invites.values().filter(|k| k.kind == KeyKind::Assistant(c) && k.expires > now).count());
        if used >= MAX_ASSISTANTS {
            out.push(Out::To(conn, reply(map(vec![("reason", "limit".into())]))));
            return;
        }
        let (Some(r), Some(secret)) = (rand_hex(4), crypto::random(32)) else { return };
        let kid = format!("a:{c}:{r}");
        let context = format!("assistant:{}", self.id);
        self.invites.insert(
            kid.clone(),
            Key {
                kind: KeyKind::Assistant(c),
                secret: secret.clone(),
                context: context.clone(),
                one_time: true,
                ttl: ASSISTANT_KEY_TTL,
                expires: now.saturating_add(ASSISTANT_KEY_TTL),
                wrong: 0,
            },
        );
        out.push(Out::To(conn, reply(map(vec![("kid", kid.into()), ("secret", Value::Bin(secret)), ("context", context.into())]))));
    }

    /// A person's assistants leave with them: `removed {reason: person}`; unused keys go.
    fn remove_assistants(&mut self, person: u32, now: Ms, out: &mut Vec<Out>) {
        self.invites.retain(|_, k| k.kind != KeyKind::Assistant(person));
        let theirs: Vec<u32> = self.clients.values().filter(|x| x.of == person && x.state != State::Gone).map(|x| x.id).collect();
        for a in &theirs {
            if let Some(conn) = self.clients.get(a).and_then(|x| x.conn) {
                out.push(Out::To(conn, Envelope::new("removed").body(map(vec![("reason", "person".into())]))));
            }
            self.drop_client(*a, now, true, out);
        }
        if !theirs.is_empty() {
            self.rotate_keys(out);
        }
    }

    fn host_people(&mut self, env: &Envelope, now: Ms, out: &mut Vec<Out>) {
        let target = id32(get_u64(&env.b, "c").unwrap_or(0));
        let Some(cl) = self.clients.get(&target) else { return };
        if cl.role == Role::Host || cl.role == Role::Replica {
            return;
        }
        let (state, of, conn) = (cl.state, cl.of, cl.conn);
        match env.t.as_str() {
            "approve" if state == State::Waiting => {
                let role = get_str(&env.b, "role").and_then(Role::parse).unwrap_or(Role::Editor);
                let role = if matches!(role, Role::Host | Role::Replica) { Role::Editor } else { role };
                let role = if of != 0 && role != Role::Viewer { Role::Assistant } else { role };
                if let Some(cl) = self.clients.get_mut(&target) {
                    cl.role = role;
                }
                self.activate(target, out);
            }
            "deny" if state == State::Waiting => {
                if let Some(conn) = conn {
                    out.push(Out::To(conn, Envelope::new("deny").body(map(vec![("reason", "host".into())]))));
                    out.push(Out::Close(conn, "denied".into()));
                    self.conns.remove(&conn);
                }
                self.clients.remove(&target);
            }
            "remove" if state != State::Gone => {
                if let Some(conn) = conn {
                    out.push(Out::To(conn, Envelope::new("removed").body(map(vec![("reason", "host".into())]))));
                }
                self.drop_client(target, now, true, out);
                self.rotate_keys(out);
            }
            "role" if state == State::Active => {
                let role = get_str(&env.b, "role").and_then(Role::parse).unwrap_or(Role::Viewer);
                let role = if of != 0 && role == Role::Editor { Role::Assistant } else { role };
                if matches!(role, Role::Editor | Role::Viewer | Role::Assistant) {
                    if let Some(cl) = self.clients.get_mut(&target) {
                        cl.role = role;
                    }
                    let b = map(vec![("c", target.into()), ("state", "role".into()), ("role", role.name().into())]);
                    self.broadcast(&Envelope::new("peer").body(b), None, out);
                    if role == Role::Viewer && of == 0 {
                        self.remove_assistants(target, now, out);
                    }
                }
            }
            _ => {}
        }
    }

    // ---------------------------------------------------------------------------------------------------------------
    // Host handoff: handover → the state to the new host → its `moved` → everyone follows with a ticket.

    fn on_handover(&mut self, host: u32, env: &Envelope, out: &mut Vec<Out>) {
        if self.handover != Handover::None {
            return;
        }
        let to = id32(get_u64(&env.b, "to").unwrap_or(0));
        let ok = self
            .clients
            .get(&to)
            .is_some_and(|c| c.id != host && c.state == State::Active && matches!(c.role, Role::Editor | Role::Viewer));
        if !ok {
            let b = map(vec![("to", to.into()), ("reason", "unknown".into())]);
            self.send(host, Envelope::new("handover-off").body(b), out);
            return;
        }
        self.handover = Handover::Waiting(to);
        let st = self.handover_state(to);
        let Ok(bytes) = crate::msgpack::pack(&st) else { return };
        let relay = self.invites.values().any(|k| k.kind == KeyKind::Link)
            || self.clients.values().any(|c| c.state == State::Active && c.via == "relay");
        let chunks: Vec<&[u8]> = bytes.chunks(HANDOVER_CHUNK).collect();
        let of = chunks.len();
        for (i, part) in chunks.into_iter().enumerate() {
            let mut b = vec![("from", host.into()), ("i", vz(i)), ("of", vz(of)), ("data", Value::Bin(part.to_vec()))];
            if i == 0 {
                b.push(("bytes", vz(bytes.len())));
                b.push(("relay", relay.into()));
            }
            self.send(to, Envelope::new("handover").body(map(b)), out);
        }
    }

    fn on_moved(&mut self, c: u32, env: &Envelope, out: &mut Vec<Out>) {
        if self.handover != Handover::Waiting(c) {
            return;
        }
        let addrs: Vec<String> = get_arr(&env.b, "addrs")
            .iter()
            .filter_map(Value::as_str)
            .filter(|a| !a.is_empty() && a.len() <= 64)
            .take(16)
            .map(String::from)
            .collect();
        let port = get_u64(&env.b, "port").unwrap_or(0);
        if addrs.is_empty() || port == 0 || port > 65_535 {
            return;
        }
        self.handover = Handover::Done;
        let room = get_str(&env.b, "room").filter(|r| r.len() <= 64).map(String::from);
        let targets: Vec<(ConnId, bool, Vec<u8>)> = self
            .clients
            .values()
            .filter(|x| x.id != c && x.state == State::Active && x.role != Role::Replica)
            .filter_map(|x| x.conn.map(|k| (k, self.conns.get(&k).is_none_or(|cx| cx.sealed), x.ticket.clone())))
            .collect();
        for (conn, sealed, ticket) in targets {
            let mut pairs = vec![("addrs", strs(&addrs)), ("port", port.into()), ("host", c.into())];
            if let Some(r) = &room {
                pairs.push(("room", r.as_str().into()));
            }
            if !sealed && !ticket.is_empty() {
                pairs.push(("ticket", Value::Bin(ticket)));
            }
            out.push(Out::To(conn, Envelope::new("moved").body(map(pairs))));
        }
    }

    /// Someone was removed: every remaining encrypted connection's frame keys rotate.
    fn rotate_keys(&mut self, out: &mut Vec<Out>) {
        let mut sealed: Vec<ConnId> = self.conns.iter().filter(|(_, cx)| cx.sealed).map(|(k, _)| *k).collect();
        sealed.sort_unstable();
        for conn in sealed {
            out.push(Out::Rekey(conn));
            out.push(Out::To(conn, Envelope::new("rekey")));
        }
    }

    fn end(&mut self, reason: &str, out: &mut Vec<Out>) {
        if self.ended {
            return;
        }
        let b = map(vec![("reason", reason.into()), ("q", self.seq.into())]);
        self.broadcast(&Envelope::new("end").body(b), None, out);
        for cl in self.clients.values_mut() {
            if let Some(conn) = cl.conn.take() {
                out.push(Out::Close(conn, "ended".into()));
            }
            cl.state = State::Gone;
        }
        let mut rest: Vec<ConnId> = self.conns.iter().filter(|(_, cx)| cx.client.is_none()).map(|(k, _)| *k).collect();
        rest.sort_unstable();
        for conn in rest {
            out.push(Out::Close(conn, "ended".into()));
        }
        self.conns.clear();
        self.invites.clear();
        self.ended = true;
    }

    /// The host ends the session from its own window (as an `end` message does).
    pub fn end_now(&mut self) -> Vec<Out> {
        let mut out = vec![];
        self.end("host-ended", &mut out);
        out
    }

    // ---------------------------------------------------------------------------------------------------------------
    // Edits: rate → queue → checks → validate → commit

    fn on_edit(&mut self, c: u32, conn: ConnId, env: Envelope, now: Ms, out: &mut Vec<Out>) {
        let Some(cl) = self.clients.get_mut(&c) else { return };
        if cl.role == Role::Replica {
            return;
        }
        cl.backlog.push_back(env);
        if cl.backlog.len() > BACKLOG_REMOVE {
            self.stats.removed_flood = self.stats.removed_flood.saturating_add(1);
            out.push(Out::To(conn, Envelope::new("removed").body(map(vec![("reason", "flood".into())]))));
            self.drop_client(c, now, true, out);
            self.rotate_keys(out);
            return;
        }
        if cl.backlog.len() > BACKLOG_WARN && !cl.warned {
            cl.warned = true;
            out.push(Out::To(conn, Envelope::new("warn").body(map(vec![("reason", "flood".into())]))));
        }
        self.drain_backlog(c, now);
    }

    fn drain_backlog(&mut self, c: u32, now: Ms) {
        let Some(cl) = self.clients.get_mut(&c) else { return };
        #[allow(clippy::cast_precision_loss)]
        let dt = now.saturating_sub(cl.bucket_at) as f64 / 1000.0;
        cl.bucket = (cl.bucket + dt * RATE_PER_SEC).min(RATE_PER_SEC);
        cl.bucket_at = now;
        while cl.bucket >= 1.0 {
            let Some(env) = cl.backlog.pop_front() else { break };
            let n = env.n;
            if n <= cl.last_n {
                continue; // re-sent after a rejoin and already decided
            }
            cl.bucket -= 1.0;
            let touched = get_arr(&env.b, "touched")
                .iter()
                .take(100_000)
                .filter_map(|t| {
                    Some(Touch {
                        k: get_str(t, "k")?.chars().take(1024).collect(),
                        f: get_str(t, "f").map(|f| f.chars().take(256).collect()),
                        r: get_u64(t, "r"),
                        m: get_str(t, "m").and_then(|s| s.chars().next()).unwrap_or('s'),
                        g: get(t, "g").is_some_and(|v| as_bool(v) == Some(true) || as_u64(v).unwrap_or(0) > 0),
                    })
                })
                .collect();
            self.queue.push_back(Queued {
                c,
                n,
                label: get_str(&env.b, "label").unwrap_or("").chars().take(200).collect(),
                ops: get(&env.b, "ops").cloned().unwrap_or(Value::Arr(vec![])),
                touched,
            });
            cl.last_n = n;
        }
        if cl.backlog.len() < BACKLOG_WARN / 2 {
            cl.warned = false;
        }
    }

    fn pump(&mut self, out: &mut Vec<Out>) {
        if self.handover != Handover::None {
            return;
        }
        while self.validating.is_none() {
            let Some(e) = self.queue.pop_front() else { return };
            if let Err((reason, by)) = self.check(&e) {
                self.refuse(&e, reason, by, out);
                continue;
            }
            match self.replica {
                Some(r) => {
                    let b = map(vec![("c", e.c.into()), ("n", e.n.into()), ("label", e.label.clone().into()), ("ops", e.ops.clone())]);
                    let mut env = Envelope::new("validate").body(b);
                    env.q = self.seq.saturating_add(1);
                    self.send(r, env, out);
                    self.validating = Some(e);
                }
                None => self.commit(e, out),
            }
        }
    }

    fn on_valid(&mut self, env: &Envelope, out: &mut Vec<Out>) {
        let Some(e) = self.validating.take() else { return };
        if self.handover != Handover::None {
            self.queue.push_front(e);
            return;
        }
        if get_u64(&env.b, "n") != Some(e.n) || get_u64(&env.b, "c") != Some(u64::from(e.c)) {
            self.refuse(&e, "invalid".into(), None, out);
            return;
        }
        if get_bool(&env.b, "ok") == Some(true) {
            self.commit(e, out);
        } else {
            let reason = get_str(&env.b, "reason").unwrap_or("invalid").chars().take(200).collect();
            self.refuse(&e, reason, None, out);
        }
    }

    /// Server-side rules, in order: role, locks, holds, versions (§ 6.2).
    fn check(&self, e: &Queued) -> Result<(), (String, Option<u32>)> {
        let Some(cl) = self.clients.get(&e.c) else { return Err(("gone".into(), None)) };
        if !cl.role.can_edit() {
            return Err(("role".into(), None));
        }
        if e.touched.iter().any(|t| self.locks.contains(&t.k)) {
            return Err(("locked".into(), None));
        }
        for t in &e.touched {
            if (matches!(t.m, 's' | 'd') || (t.m == 'r' && t.g))
                && let Some((h, _)) = self.holds.get(&t.k)
                && *h != e.c
            {
                return Err(("held".into(), Some(*h)));
            }
        }
        for t in &e.touched {
            let ks = self.keys.get(&t.k);
            let deleted = ks.is_some_and(|k| k.deleted);
            let by = ks.map(|k| k.by);
            match t.m {
                'p' => {
                    if deleted {
                        return Err(("deleted".into(), by));
                    }
                    if let (Some(r), Some(f)) = (t.r, &t.f) {
                        let (pr, pby) = self.props.get(&prop_key(&t.k, f)).copied().unwrap_or((0, 0));
                        if pr != r {
                            return Err(("changed".into(), Some(pby).filter(|x| *x != 0)));
                        }
                    }
                }
                'c' => {
                    if ks.is_some() && !deleted {
                        return Err(("exists".into(), by));
                    }
                }
                'd' => {
                    if deleted {
                        return Err(("deleted".into(), by));
                    }
                    if let Some(r) = t.r
                        && self.struct_rev.get(&t.k).copied().unwrap_or(0) > r
                    {
                        return Err(("changed".into(), by));
                    }
                }
                _ => {
                    if deleted {
                        return Err(("deleted".into(), by));
                    }
                    if let Some(r) = t.r
                        && ks.map_or(0, |k| k.rev) != r
                    {
                        return Err(("changed".into(), by));
                    }
                }
            }
        }
        Ok(())
    }

    fn refuse(&mut self, e: &Queued, reason: String, by: Option<u32>, out: &mut Vec<Out>) {
        self.stats.refused = self.stats.refused.saturating_add(1);
        let mut pairs = vec![("n", Value::from(e.n)), ("reason", Value::from(reason))];
        if let Some(by) = by {
            pairs.push(("by", by.into()));
            if let Some(p) = self.clients.get(&by) {
                pairs.push(("by_name", p.name.clone().into()));
            }
        }
        let body = map(pairs);
        if let Some(cl) = self.clients.get_mut(&e.c) {
            cl.refusals.push_back((e.n, body.clone()));
            while cl.refusals.len() > 1000 {
                cl.refusals.pop_front();
            }
        }
        let mut env = Envelope::new("refuse").body(body);
        env.n = e.n;
        self.send(e.c, env, out);
    }

    fn commit(&mut self, e: Queued, out: &mut Vec<Out>) {
        self.seq = self.seq.saturating_add(1);
        let q = self.seq;
        for t in &e.touched {
            match t.m {
                'r' => {}
                'p' => {
                    let ks = self.keys.entry(t.k.clone()).or_default();
                    ks.rev = q;
                    ks.by = e.c;
                    if let Some(f) = &t.f {
                        self.props.insert(prop_key(&t.k, f), (q, e.c));
                    }
                }
                m => {
                    let ks = self.keys.entry(t.k.clone()).or_default();
                    ks.rev = q;
                    ks.by = e.c;
                    ks.deleted = m == 'd';
                    if m == 'c' {
                        let pre = format!("{}\u{0}", t.k);
                        self.props.retain(|k, _| !k.starts_with(&pre));
                    }
                    self.struct_rev.insert(t.k.clone(), q);
                    if m == 'd' && self.holds.remove(&t.k).is_some() {
                        self.broadcast(&holds_change(&[], std::slice::from_ref(&t.k)), None, out);
                    }
                }
            }
        }
        let touched = Value::Arr(
            e.touched
                .iter()
                .map(|t| {
                    let mut p = vec![("k", Value::from(t.k.as_str())), ("m", Value::from(t.m.to_string()))];
                    if let Some(f) = &t.f {
                        p.push(("f", f.as_str().into()));
                    }
                    map(p)
                })
                .collect(),
        );
        let cm = Commit { q, c: e.c, n: e.n, label: e.label, ops: e.ops, touched };
        self.stats.commits = self.stats.commits.saturating_add(1);
        let env = commit_env(&cm);
        self.broadcast(&env, None, out);
        self.log.push_back(cm);
        while self.log.len() > self.cfg.log_keep {
            self.log.pop_front();
        }
    }

    fn on_snap(&mut self, env: &Envelope, out: &mut Vec<Out>) {
        let Some(target) = self.snap_busy else { return };
        if get_u64(&env.b, "for") != Some(u64::from(target)) {
            return;
        }
        let q = get_u64(&env.b, "q").unwrap_or(self.seq).min(self.seq);
        let data: &[u8] = get(&env.b, "data").and_then(bin).unwrap_or(&[]);
        let enc = get_str(&env.b, "enc").unwrap_or("").chars().take(16).collect::<String>();
        let mut known: Vec<(&String, u64)> = self.keys.iter().filter(|(_, k)| k.rev > 0 && k.rev <= q).map(|(k, v)| (k, v.rev)).collect();
        known.sort();
        let revs: Vec<Envelope> = known
            .chunks(REVS_CHUNK)
            .map(|part| {
                let m: Vec<(String, Value)> = part.iter().map(|(k, r)| ((*k).clone(), Value::from(*r))).collect();
                let mut e = Envelope::new("revs").body(map(vec![("keys", Value::Map(m))]));
                e.q = q;
                e
            })
            .collect();
        for e in revs {
            self.send(target, e, out);
        }
        let parts: Vec<&[u8]> = if data.is_empty() { vec![&[][..]] } else { data.chunks(SNAPSHOT_CHUNK).collect() };
        let total = parts.len();
        for (i, part) in parts.into_iter().enumerate() {
            let mut pairs = vec![("i", vz(i)), ("of", vz(total)), ("data", Value::Bin(part.to_vec()))];
            if i == 0 {
                pairs.push(("enc", enc.clone().into()));
                pairs.push(("bytes", vz(data.len())));
            }
            let mut e = Envelope::new("snap").body(map(pairs));
            e.q = q;
            self.send(target, e, out);
        }
        let after: Vec<Envelope> = self.log.iter().filter(|x| x.q > q).map(commit_env).collect();
        for e in after {
            self.send(target, e, out);
        }
        self.snap_busy = None;
        self.pump_snapshots(out);
    }

    // ---------------------------------------------------------------------------------------------------------------
    // Time

    pub fn tick(&mut self, now: Ms) -> Vec<Out> {
        self.now = now;
        let mut out = vec![];
        if self.ended {
            return out;
        }
        let ids: Vec<u32> = self.clients.iter().filter(|(_, c)| !c.backlog.is_empty()).map(|(id, _)| *id).collect();
        for c in ids {
            self.drain_backlog(c, now);
        }
        self.pump(&mut out);
        if now.saturating_sub(self.last_presence) >= PRESENCE_EVERY {
            self.last_presence = now;
            let dirty: Vec<(u32, Value)> = self
                .clients
                .values_mut()
                .filter(|c| c.presence_dirty && c.state == State::Active)
                .map(|c| {
                    c.presence_dirty = false;
                    (c.id, c.presence.clone())
                })
                .collect();
            if !dirty.is_empty() {
                let targets: Vec<(u32, ConnId)> = self
                    .clients
                    .values()
                    .filter(|c| c.state == State::Active && c.role != Role::Replica)
                    .filter_map(|c| c.conn.map(|k| (c.id, k)))
                    .collect();
                for (id, conn) in targets {
                    let peers: Vec<(String, Value)> =
                        dirty.iter().filter(|(c, _)| *c != id).map(|(c, v)| (c.to_string(), v.clone())).collect();
                    if peers.is_empty() {
                        continue;
                    }
                    self.stats.presence_out = self.stats.presence_out.saturating_add(1);
                    out.push(Out::To(conn, Envelope::new("presence").body(map(vec![("peers", Value::Map(peers))]))));
                }
            }
        }
        if now.saturating_sub(self.last_ping) >= PING_EVERY {
            self.last_ping = now;
            let mut conns: Vec<ConnId> = self.conns.iter().filter(|(_, cx)| cx.client.is_some()).map(|(k, _)| *k).collect();
            conns.sort_unstable();
            for conn in conns {
                let mut e = Envelope::new("ping");
                e.n = now;
                out.push(Out::To(conn, e));
            }
        }
        let late: Vec<u32> = self
            .clients
            .values()
            .filter(|c| c.state != State::Gone && c.conn.is_some() && now.saturating_sub(c.last_seen) > GONE_AFTER)
            .filter(|c| !c.conn.is_some_and(|k| self.conns.get(&k).is_some_and(|x| x.trusted)))
            .map(|c| c.id)
            .collect();
        for c in late {
            self.drop_client(c, now, false, &mut out);
        }
        let stale: Vec<(ConnId, Option<String>)> = self
            .conns
            .iter()
            .filter(|(_, cx)| cx.client.is_none() && !cx.trusted)
            .filter(|(_, cx)| match &cx.hs {
                Some(h) => now.saturating_sub(h.started) > HANDSHAKE_TIMEOUT,
                None => now.saturating_sub(cx.opened) > HANDSHAKE_TIMEOUT,
            })
            .map(|(k, cx)| (*k, cx.hs.as_ref().map(|h| h.kid.clone())))
            .collect();
        for (conn, kid) in stale {
            self.conns.remove(&conn);
            out.push(Out::Close(conn, "timeout".into()));
            if let Some(kid) = kid {
                self.wrong_try(&kid, now, &mut out);
            }
        }
        let expired: Vec<String> = self.holds.iter().filter(|(_, (_, t))| *t < now).map(|(k, _)| k.clone()).collect();
        if !expired.is_empty() {
            for k in &expired {
                self.holds.remove(k);
            }
            self.broadcast(&holds_change(&[], &expired), None, &mut out);
        }
        let old: Vec<String> = self.invites.iter().filter(|(_, k)| k.expires <= now).map(|(k, _)| k.clone()).collect();
        for k in old {
            let quiet = matches!(self.invites.remove(&k), Some(Key { kind: KeyKind::Assistant(_), .. }));
            if !quiet {
                self.to_role(Role::Host, code_gone(&k, "expired"), &mut out);
            }
        }
        let forgotten: Vec<u32> = self
            .clients
            .values()
            .filter(|c| c.state == State::Gone && now.saturating_sub(c.gone_at) > REJOIN_WINDOW)
            .map(|c| c.id)
            .collect();
        for c in forgotten {
            self.clients.remove(&c);
            self.invites.remove(&ticket_kid(c));
        }
        out
    }

    // ---------------------------------------------------------------------------------------------------------------
    // Sending

    fn send(&self, c: u32, env: Envelope, out: &mut Vec<Out>) {
        if let Some(conn) = self.clients.get(&c).and_then(|x| x.conn) {
            out.push(Out::To(conn, env));
        }
    }
    fn role_client(&self, role: Role) -> Option<u32> {
        self.clients.values().find(|c| c.role == role && c.state == State::Active).map(|c| c.id)
    }
    fn to_role(&self, role: Role, env: Envelope, out: &mut Vec<Out>) {
        for c in self.clients.values().filter(|c| c.role == role && c.state == State::Active) {
            if let Some(conn) = c.conn {
                out.push(Out::To(conn, env.clone()));
            }
        }
    }
    /// To every active client (the replica too: it needs commits, holds and locks).
    fn broadcast(&self, env: &Envelope, except: Option<u32>, out: &mut Vec<Out>) {
        for c in self.clients.values().filter(|c| c.state == State::Active && Some(c.id) != except) {
            if let Some(conn) = c.conn {
                out.push(Out::To(conn, env.clone()));
            }
        }
    }
}
