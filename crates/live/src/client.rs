//! A Live client without a screen (live-client.js's core, docs/LIVE-PROTOCOL.md § 6–8): numbers its edits, keeps them
//! pending until the server decides, applies commits in order (its own confirm a pending edit; others' go to the app,
//! which rebases what is still pending over them), learns key versions from commits and `revs`, and fills the version
//! conditions (`r`) of its structural edits and undos from them. Edits travel as **op records** (AGENTS.md § 6):
//! `{command, params, base_revision, result_revision}`, never snapshots.

use std::collections::{HashMap, VecDeque};

use crate::envelope::Envelope;
use crate::msgpack::Value;
use crate::session::Touch;

/// One journaled edit as it travels: the command and its parameters, the revision it was made on and the one it made.
#[derive(Debug, Clone, PartialEq)]
pub struct OpRecord {
    pub command: String,
    pub params: Value,
    pub base_revision: u64,
    pub result_revision: u64,
}

impl OpRecord {
    pub fn to_value(&self) -> Value {
        Value::map(vec![
            ("command", Value::Str(self.command.clone())),
            ("params", self.params.clone()),
            ("base_revision", Value::from(self.base_revision)),
            ("result_revision", Value::from(self.result_revision)),
        ])
    }
    pub fn from_value(v: &Value) -> Option<OpRecord> {
        Some(OpRecord {
            command: v.get("command")?.as_str()?.to_string(),
            params: v.get("params").cloned().unwrap_or(Value::Nil),
            base_revision: v.get("base_revision").and_then(Value::as_u64).unwrap_or(0),
            result_revision: v.get("result_revision").and_then(Value::as_u64).unwrap_or(0),
        })
    }
}

/// An edit waiting for the server.
#[derive(Debug, Clone, PartialEq)]
pub struct Pending {
    pub n: u64,
    pub label: String,
    pub ops: Vec<OpRecord>,
    pub touched: Vec<Touch>,
}

/// What applying a message means for the app.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// someone else's committed edit: apply these ops (then rebase what is pending)
    Remote { c: u64, q: u64, label: String, ops: Vec<OpRecord> },
    /// our own edit committed (it was already applied locally)
    Confirmed { n: u64, q: u64 },
    /// our edit was refused: undo it locally
    Refused { n: u64, reason: String, by: Option<u64>, by_name: Option<String> },
    /// we are in: the client id, our id prefix, role, and whether a snapshot follows
    Welcome { c: u64, role: String, catchup: String },
    /// the holds table changed
    Holds,
    /// the session ended (reason)
    Ended(String),
}

/// The client core.
#[derive(Debug, Default)]
pub struct Client {
    pub c: u64,
    pub role: String,
    pub prefix: String,
    /// the last seq applied
    pub seq: u64,
    next_n: u64,
    pub pending: VecDeque<Pending>,
    revs: HashMap<String, u64>,
    prop_revs: HashMap<(String, String), u64>,
    /// key → holder
    pub holds: HashMap<String, u64>,
    pub token: String,
}

fn ops_of(v: Option<&Value>) -> Vec<OpRecord> {
    v.map(Value::as_arr).unwrap_or(&[]).iter().filter_map(OpRecord::from_value).collect()
}

impl Client {
    pub fn new() -> Self {
        Client { next_n: 1, ..Default::default() }
    }

    /// The version of a key as this client knows it (§ 6.2 `rev(k)`).
    pub fn rev(&self, k: &str) -> u64 {
        self.revs.get(k).copied().unwrap_or(0)
    }
    /// The seq of the last write of a property.
    pub fn prop_rev(&self, k: &str, f: &str) -> u64 {
        self.prop_revs.get(&(k.to_string(), f.to_string())).copied().unwrap_or(0)
    }

    /// A local step the app made: numbered and kept pending (the adapter gives each `touched` its version condition
    /// `r` where it wants one, from [`Client::rev`] / [`Client::prop_rev`]). → the `edit` envelope to send.
    pub fn edit(&mut self, label: &str, ops: Vec<OpRecord>, touched: Vec<Touch>) -> Envelope {
        let n = self.next_n;
        self.next_n = self.next_n.saturating_add(1);
        let env = Self::edit_env(n, label, &ops, &touched);
        self.pending.push_back(Pending { n, label: label.to_string(), ops, touched });
        env
    }

    fn edit_env(n: u64, label: &str, ops: &[OpRecord], touched: &[Touch]) -> Envelope {
        let tv: Vec<Value> = touched
            .iter()
            .map(|t| {
                let mut m = vec![("k".to_string(), Value::Str(t.k.clone())), ("m".to_string(), Value::Str(t.m.to_string()))];
                if let Some(f) = &t.f {
                    m.push(("f".into(), Value::Str(f.clone())));
                }
                if let Some(r) = t.r {
                    m.push(("r".into(), Value::from(r)));
                }
                if t.g {
                    m.push(("g".into(), Value::from(1u64)));
                }
                Value::Map(m)
            })
            .collect();
        let mut e = Envelope::new("edit").body(Value::map(vec![
            ("label", Value::Str(label.to_string())),
            ("ops", Value::Arr(ops.iter().map(OpRecord::to_value).collect())),
            ("touched", Value::Arr(tv)),
        ]));
        e.n = n;
        e
    }

    /// After (re)connecting and `welcome`: the pending edits again, in order (§ 6.4).
    pub fn resend(&self) -> Vec<Envelope> {
        self.pending.iter().map(|p| Self::edit_env(p.n, &p.label, &p.ops, &p.touched)).collect()
    }

    /// `hello` for this client (`resume` when it has been in before).
    pub fn hello(&self, app: &str, schema: u64, name: &str) -> Envelope {
        let mut b = vec![
            ("v", Value::from(crate::envelope::PROTOCOL)),
            ("app", Value::from(app)),
            ("schema", Value::from(schema)),
            ("name", Value::from(name)),
        ];
        if self.c != 0 {
            let holds: Vec<Value> = self.holds.iter().filter(|(_, h)| **h == self.c).map(|(k, _)| Value::Str(k.clone())).collect();
            b.push((
                "resume",
                Value::map(vec![
                    ("c", Value::from(self.c)),
                    ("token", Value::Str(self.token.clone())),
                    ("q", Value::from(self.seq)),
                    ("n", Value::from(self.pending.front().map_or(self.next_n, |p| p.n))),
                    ("holds", Value::Arr(holds)),
                ]),
            ));
        }
        Envelope::new("hello").body(Value::map(b))
    }

    fn learn(&mut self, q: u64, touched: &[Value]) {
        for t in touched.iter().filter_map(Touch::from_value) {
            if t.m == 'r' {
                continue;
            }
            self.revs.insert(t.k.clone(), q);
            if t.m == 'c' {
                self.prop_revs.retain(|(k, _), _| *k != t.k);
            }
            if let (Some(f), 'p') = (&t.f, t.m) {
                self.prop_revs.insert((t.k.clone(), f.clone()), q);
            }
        }
    }

    /// One message from the server → what it means for the app.
    pub fn receive(&mut self, env: &Envelope) -> Vec<Event> {
        match env.t.as_str() {
            "welcome" => {
                self.c = env.b.get("c").and_then(Value::as_u64).unwrap_or(self.c);
                self.role = env.b.get("role").and_then(Value::as_str).unwrap_or("").to_string();
                self.prefix = env.b.get("prefix").and_then(Value::as_str).unwrap_or("").to_string();
                self.token = env.b.get("token").and_then(Value::as_str).unwrap_or("").to_string();
                if let Some(Value::Map(h)) = env.b.get("holds") {
                    self.holds = h.iter().filter_map(|(k, v)| v.as_u64().map(|c| (k.clone(), c))).collect();
                }
                // a snapshot joiner starts at the snapshot's seq; a log catch-up at the seq it sent
                if self.seq == 0 {
                    self.seq = env.b.get("saved").and_then(|s| s.get("q")).and_then(Value::as_u64).unwrap_or(0);
                }
                let catchup = env.b.get("catchup").and_then(Value::as_str).unwrap_or("").to_string();
                vec![Event::Welcome { c: self.c, role: self.role.clone(), catchup }]
            }
            "revs" => {
                if let Some(Value::Map(keys)) = env.b.get("keys") {
                    for (k, v) in keys {
                        if let Some(q) = v.as_u64() {
                            self.revs.insert(k.clone(), q);
                        }
                    }
                }
                self.seq = self.seq.max(env.q);
                Vec::new()
            }
            "commit" => {
                if env.q <= self.seq {
                    return Vec::new(); // already applied (a catch-up overlapping what we had)
                }
                self.seq = env.q;
                let touched = env.b.get("touched").map(Value::as_arr).unwrap_or(&[]).to_vec();
                self.learn(env.q, &touched);
                if env.c == self.c && self.c != 0 {
                    self.pending.retain(|p| p.n != env.n);
                    vec![Event::Confirmed { n: env.n, q: env.q }]
                } else {
                    let label = env.b.get("label").and_then(Value::as_str).unwrap_or("").to_string();
                    vec![Event::Remote { c: env.c, q: env.q, label, ops: ops_of(env.b.get("ops")) }]
                }
            }
            "refuse" => {
                let n = env.b.get("n").and_then(Value::as_u64).unwrap_or(env.n);
                self.pending.retain(|p| p.n != n);
                vec![Event::Refused {
                    n,
                    reason: env.b.get("reason").and_then(Value::as_str).unwrap_or("").to_string(),
                    by: env.b.get("by").and_then(Value::as_u64),
                    by_name: env.b.get("by_name").and_then(Value::as_str).map(str::to_string),
                }]
            }
            "holds" => {
                if let Some(Value::Map(set)) = env.b.get("set") {
                    for (k, v) in set {
                        if let Some(c) = v.as_u64() {
                            self.holds.insert(k.clone(), c);
                        }
                    }
                }
                for k in env.b.get("clear").map(Value::as_arr).unwrap_or(&[]) {
                    if let Some(k) = k.as_str() {
                        self.holds.remove(k);
                    }
                }
                vec![Event::Holds]
            }
            "end" => vec![Event::Ended(env.b.get("reason").and_then(Value::as_str).unwrap_or("").to_string())],
            _ => Vec::new(),
        }
    }
}
