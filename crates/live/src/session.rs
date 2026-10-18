//! The session core (docs/LIVE-PROTOCOL.md § 6, § 9): who may edit, locks, holds, the order, and the rules that decide
//! each edit from its `touched` keys — the same decisions as the session server in Studio (desktop-shell
//! `live/session.rs` `check` / `commit`). It works on envelopes and never reads an app's ops (they come back unchanged
//! in `commit`). No sockets, no handshake: a transport (or a test) feeds it envelopes per client and sends what it returns.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::envelope::Envelope;
use crate::msgpack::Value;

/// A role (§ 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Host,
    Editor,
    Viewer,
    Assistant,
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
            "editor" => Role::Editor,
            "viewer" => Role::Viewer,
            "assistant" => Role::Assistant,
            "replica" => Role::Replica,
            _ => return None,
        })
    }
    fn can_edit(self) -> bool {
        matches!(self, Role::Host | Role::Editor | Role::Assistant)
    }
}

/// One `touched` entry: `{k, m, f?, r?, g?}` (§ 6.2).
#[derive(Debug, Clone, PartialEq)]
pub struct Touch {
    pub k: String,
    /// p (property write), s (structural), c (create), d (delete), r (read)
    pub m: char,
    pub f: Option<String>,
    pub r: Option<u64>,
    /// a guarded read: refused while someone else holds `k`
    pub g: bool,
}

impl Touch {
    /// From a `touched` item; None when it is not one (no key, or an unknown mode).
    pub fn from_value(v: &Value) -> Option<Touch> {
        let k = v.get("k")?.as_str()?.to_string();
        let m = v.get("m")?.as_str()?.chars().next()?;
        if !matches!(m, 'p' | 's' | 'c' | 'd' | 'r') || k.is_empty() || k.len() > 1024 {
            return None;
        }
        let truthy = |x: Option<&Value>| match x {
            Some(Value::Bool(b)) => *b,
            Some(Value::Int(i)) => *i != 0,
            Some(Value::F64(f)) => *f != 0.0,
            _ => false,
        };
        Some(Touch {
            k,
            m,
            f: v.get("f").and_then(Value::as_str).map(str::to_string),
            r: v.get("r").and_then(Value::as_u64),
            g: truthy(v.get("g")),
        })
    }
    /// What a commit carries back: `{k, m, f?}`.
    pub fn bare(&self) -> Value {
        let mut m = vec![("k".to_string(), Value::Str(self.k.clone())), ("m".to_string(), Value::Str(self.m.to_string()))];
        if let Some(f) = &self.f {
            m.push(("f".into(), Value::Str(f.clone())));
        }
        Value::Map(m)
    }
}

#[derive(Debug, Clone, Default)]
struct KeyState {
    rev: u64,
    by: u64,
    deleted: bool,
}

#[derive(Debug, Clone)]
struct Client {
    name: String,
    role: Role,
    /// the highest edit counter decided for this client
    last_n: u64,
}

/// A decision.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    Commit {
        q: u64,
    },
    Refuse {
        reason: String,
        by: Option<u64>,
    },
    /// already decided (n ≤ last_n): ignored
    Ignored,
}

/// What the core sends: to one client, or to every active one.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    To(u64, Envelope),
    All(Envelope),
}

/// Limits (§ 10).
pub const MAX_EDIT_BYTES: usize = 1024 * 1024;
pub const HOLD_MS: u64 = 30_000;

/// The session's decisions.
#[derive(Debug, Default)]
pub struct Core {
    clients: BTreeMap<u64, Client>,
    next_c: u64,
    seq: u64,
    keys: HashMap<String, KeyState>,
    /// (key, field) → (seq, by) of its last property write
    props: HashMap<(String, String), (u64, u64)>,
    struct_rev: HashMap<String, u64>,
    /// key → (holder, expires at)
    holds: HashMap<String, (u64, u64)>,
    locks: HashSet<String>,
    /// committed edits, oldest first (the log a rejoin catches up from)
    pub log: Vec<Envelope>,
}

const LOG_KEEP: usize = 10_000;

impl Core {
    pub fn new() -> Self {
        Core { next_c: 1, ..Default::default() }
    }
    pub fn seq(&self) -> u64 {
        self.seq
    }
    /// A client in the session (approved, or attached in-process) → its id.
    pub fn add(&mut self, name: &str, role: Role) -> u64 {
        let c = self.next_c;
        self.next_c = self.next_c.saturating_add(1);
        self.clients.insert(c, Client { name: name.chars().take(80).collect(), role, last_n: 0 });
        c
    }
    pub fn role(&self, c: u64) -> Option<Role> {
        self.clients.get(&c).map(|x| x.role)
    }
    /// `role` from the host.
    pub fn set_role(&mut self, by: u64, c: u64, role: Role) -> Result<(), &'static str> {
        if self.role(by) != Some(Role::Host) {
            return Err("only the host changes roles");
        }
        if matches!(role, Role::Host | Role::Replica) {
            return Err("that role is not given");
        }
        let cl = self.clients.get_mut(&c).ok_or("no such client")?;
        cl.role = role;
        Ok(())
    }
    pub fn remove(&mut self, c: u64) {
        self.clients.remove(&c);
        self.holds.retain(|_, (h, _)| *h != c);
    }
    /// The key's version (§ 6.2 `rev(k)`).
    pub fn rev(&self, k: &str) -> u64 {
        self.keys.get(k).map_or(0, |s| s.rev)
    }

    /// `hold {keys}`: keys someone else holds are skipped; yours are refreshed. → the `holds` change to broadcast.
    pub fn hold(&mut self, c: u64, keys: &[String], now: u64) -> Option<Envelope> {
        let mut set = Vec::new();
        for k in keys.iter().take(10_000) {
            match self.holds.get(k) {
                Some((h, exp)) if *h != c && *exp > now => continue,
                _ => {
                    self.holds.insert(k.clone(), (c, now.saturating_add(HOLD_MS)));
                    set.push((k.clone(), Value::from(c)));
                }
            }
        }
        (!set.is_empty())
            .then(|| Envelope::new("holds").body(Value::map(vec![("set", Value::Map(set)), ("clear", Value::Arr(Vec::new()))])))
    }
    /// `release {keys}` / `{all: true}`.
    pub fn release(&mut self, c: u64, keys: Option<&[String]>) -> Option<Envelope> {
        let gone: Vec<String> = match keys {
            None => self.holds.iter().filter(|(_, (h, _))| *h == c).map(|(k, _)| k.clone()).collect(),
            Some(ks) => ks.iter().filter(|k| self.holds.get(*k).is_some_and(|(h, _)| *h == c)).cloned().collect(),
        };
        for k in &gone {
            self.holds.remove(k);
        }
        (!gone.is_empty()).then(|| {
            Envelope::new("holds")
                .body(Value::map(vec![("set", Value::Map(Vec::new())), ("clear", Value::Arr(gone.into_iter().map(Value::Str).collect()))]))
        })
    }
    /// Expire holds older than 30 s.
    pub fn tick(&mut self, now: u64) -> Option<Envelope> {
        let gone: Vec<String> = self.holds.iter().filter(|(_, (_, exp))| *exp <= now).map(|(k, _)| k.clone()).collect();
        for k in &gone {
            self.holds.remove(k);
        }
        (!gone.is_empty()).then(|| {
            Envelope::new("holds")
                .body(Value::map(vec![("set", Value::Map(Vec::new())), ("clear", Value::Arr(gone.into_iter().map(Value::Str).collect()))]))
        })
    }
    /// `lock` / `unlock` from the host.
    pub fn lock(&mut self, by: u64, keys: &[String], on: bool) -> Result<Envelope, &'static str> {
        if self.role(by) != Some(Role::Host) {
            return Err("only the host locks");
        }
        let mut changed = Vec::new();
        for k in keys.iter().take(10_000) {
            let did = if on { self.locks.insert(k.clone()) } else { self.locks.remove(k) };
            if did {
                changed.push(Value::Str(k.clone()));
            }
        }
        let (add, del) = if on { (changed, Vec::new()) } else { (Vec::new(), changed) };
        Ok(Envelope::new("locks").body(Value::map(vec![("add", Value::Arr(add)), ("del", Value::Arr(del))])))
    }

    /// The server's checks for one edit (desktop-shell session.rs `check`): role, locks, holds, then the key rules.
    fn check(&self, c: u64, touched: &[Touch]) -> Result<(), (String, Option<u64>)> {
        let Some(cl) = self.clients.get(&c) else { return Err(("gone".into(), None)) };
        if !cl.role.can_edit() {
            return Err(("role".into(), None));
        }
        if touched.iter().any(|t| self.locks.contains(&t.k)) {
            return Err(("locked".into(), None));
        }
        for t in touched {
            if (matches!(t.m, 's' | 'd') || (t.m == 'r' && t.g))
                && let Some((h, _)) = self.holds.get(&t.k)
                && *h != c
            {
                return Err(("held".into(), Some(*h)));
            }
        }
        for t in touched {
            let ks = self.keys.get(&t.k);
            let deleted = ks.is_some_and(|k| k.deleted);
            let by = ks.map(|k| k.by);
            match t.m {
                'p' => {
                    if deleted {
                        return Err(("deleted".into(), by));
                    }
                    if let (Some(r), Some(f)) = (t.r, &t.f) {
                        let (pr, pby) = self.props.get(&(t.k.clone(), f.clone())).copied().unwrap_or((0, 0));
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
                    // delete wins over property writes; a condition, when given, is on structure only
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

    fn apply_commit(&mut self, c: u64, touched: &[Touch]) -> u64 {
        self.seq = self.seq.saturating_add(1);
        let q = self.seq;
        for t in touched {
            match t.m {
                'r' => {}
                'p' => {
                    let ks = self.keys.entry(t.k.clone()).or_default();
                    ks.rev = q;
                    ks.by = c;
                    if let Some(f) = &t.f {
                        self.props.insert((t.k.clone(), f.clone()), (q, c));
                    }
                }
                m => {
                    let ks = self.keys.entry(t.k.clone()).or_default();
                    ks.rev = q;
                    ks.by = c;
                    ks.deleted = m == 'd';
                    if m == 'c' {
                        // a re-created key (undo of a delete) starts its properties afresh
                        self.props.retain(|(k, _), _| *k != t.k);
                    }
                    self.struct_rev.insert(t.k.clone(), q);
                    if m == 'd' {
                        self.holds.remove(&t.k);
                    }
                }
            }
        }
        q
    }

    /// Decide one edit (an `edit` envelope from client `c`) → the decision and what to send.
    pub fn edit(&mut self, c: u64, env: &Envelope) -> (Decision, Vec<Out>) {
        let n = env.n;
        let Some(cl) = self.clients.get(&c) else { return (Decision::Refuse { reason: "gone".into(), by: None }, Vec::new()) };
        if n != 0 && n <= cl.last_n {
            return (Decision::Ignored, Vec::new());
        }
        let touched: Vec<Touch> = env.b.get("touched").map(Value::as_arr).unwrap_or(&[]).iter().filter_map(Touch::from_value).collect();
        let size = crate::msgpack::pack(&env.b).map_or(usize::MAX, |b| b.len());
        if let Some(cl) = self.clients.get_mut(&c) {
            cl.last_n = cl.last_n.max(n);
        }
        let verdict = if size > MAX_EDIT_BYTES { Err(("too-large".to_string(), None)) } else { self.check(c, &touched) };
        match verdict {
            Err((reason, by)) => {
                let mut body = vec![("n", Value::from(n)), ("reason", Value::Str(reason.clone()))];
                if let Some(b) = by {
                    body.push(("by", Value::from(b)));
                    if let Some(p) = self.clients.get(&b) {
                        body.push(("by_name", Value::Str(p.name.clone())));
                    }
                }
                let mut e = Envelope::new("refuse").body(Value::map(body));
                e.n = n;
                (Decision::Refuse { reason, by }, vec![Out::To(c, e)])
            }
            Ok(()) => {
                let q = self.apply_commit(c, &touched);
                let label = env.b.get("label").cloned().unwrap_or(Value::Str(String::new()));
                let ops = env.b.get("ops").cloned().unwrap_or(Value::Nil);
                let body = Value::map(vec![
                    ("c", Value::from(c)),
                    ("n", Value::from(n)),
                    ("label", label),
                    ("ops", ops),
                    ("touched", Value::Arr(touched.iter().map(Touch::bare).collect())),
                ]);
                let e = Envelope { t: "commit".into(), s: String::new(), c, n, q, b: body };
                self.log.push(e.clone());
                if self.log.len() > LOG_KEEP {
                    self.log.remove(0);
                }
                (Decision::Commit { q }, vec![Out::All(e)])
            }
        }
    }

    /// One message from client `c` (the editing and holding part of § 4.2–4.3). → what to send.
    pub fn message(&mut self, c: u64, env: &Envelope, now: u64) -> Vec<Out> {
        let keys = || -> Vec<String> {
            env.b.get("keys").map(Value::as_arr).unwrap_or(&[]).iter().filter_map(|k| k.as_str().map(str::to_string)).collect()
        };
        match env.t.as_str() {
            "edit" => self.edit(c, env).1,
            "hold" => self.hold(c, &keys(), now).map(Out::All).into_iter().collect(),
            "release" => {
                let all = matches!(env.b.get("all"), Some(Value::Bool(true)));
                let ks = keys();
                self.release(c, if all { None } else { Some(&ks) }).map(Out::All).into_iter().collect()
            }
            "lock" | "unlock" => self.lock(c, &keys(), env.t == "lock").map(Out::All).into_iter().collect(),
            "role" => {
                let who = env.b.get("c").and_then(Value::as_u64).unwrap_or(0);
                let role = env.b.get("role").and_then(Value::as_str).and_then(Role::parse);
                match role.map(|r| self.set_role(c, who, r)) {
                    Some(Ok(())) => vec![Out::All(Envelope::new("peer").body(Value::map(vec![
                        ("c", Value::from(who)),
                        ("state", "role".into()),
                        ("role", Value::from(role.map_or("", Role::name))),
                    ])))],
                    _ => Vec::new(),
                }
            }
            "ping" => {
                let mut e = Envelope::new("pong");
                e.n = env.n;
                vec![Out::To(c, e)]
            }
            // unknown types are ignored (§ 2)
            _ => Vec::new(),
        }
    }

    /// The commits after seq `from` (a rejoin's catch-up), if the log still reaches back.
    pub fn since(&self, from: u64) -> Option<Vec<Envelope>> {
        let first = self.log.first().map_or(self.seq.saturating_add(1), |e| e.q);
        if from.saturating_add(1) < first && from < self.seq {
            return None;
        }
        Some(self.log.iter().filter(|e| e.q > from).cloned().collect())
    }
}
