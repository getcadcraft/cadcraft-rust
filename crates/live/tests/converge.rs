//! Two people and the host editing through the session core, every message through the wire format: edits travel as op
//! records, each side applies commits in order, and they end with the same document (docs/LIVE-PROTOCOL.md § 6).

use std::collections::BTreeMap;

use cc_live::client::{Client, Event, OpRecord};
use cc_live::envelope::Envelope;
use cc_live::msgpack::Value;
use cc_live::session::{Core, Out, Role, Touch};

/// A toy document: key → colour; ops `set {k, v}`, `del {k}`.
#[derive(Default, Debug, PartialEq, Clone)]
struct Doc(BTreeMap<String, String>);

impl Doc {
    fn apply(&mut self, op: &OpRecord) {
        let k = op.params.get("k").and_then(Value::as_str).unwrap_or("").to_string();
        match op.command.as_str() {
            "set" => {
                self.0.insert(k, op.params.get("v").and_then(Value::as_str).unwrap_or("").to_string());
            }
            "del" => {
                self.0.remove(&k);
            }
            _ => {}
        }
    }
}

fn wire(e: &Envelope) -> Envelope {
    Envelope::decode(&e.encode().unwrap()).unwrap()
}

fn set(k: &str, v: &str, base: u64) -> OpRecord {
    OpRecord {
        command: "set".into(),
        params: Value::map(vec![("k", k.into()), ("v", v.into())]),
        base_revision: base,
        result_revision: base + 1,
    }
}

#[test]
fn edits_converge() {
    let mut core = Core::new();
    let names = ["Host", "Ana", "Ben"];
    let mut clients: Vec<(u64, Client, Doc)> = names
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let c = core.add(n, if i == 0 { Role::Host } else { Role::Editor });
            let mut cl = Client::new();
            let w = Envelope::new("welcome").body(Value::map(vec![
                ("c", Value::from(c)),
                ("role", "editor".into()),
                ("catchup", "none".into()),
            ]));
            cl.receive(&wire(&w));
            (c, cl, Doc::default())
        })
        .collect();
    // each person edits locally (applied at once), sends the edit; the core decides; everyone applies what comes back
    let plan = [(1usize, "e:1", "red"), (2, "e:1", "blue"), (1, "e:2", "green"), (0, "e:3", "black"), (2, "e:2", "white")];
    let mut outs: Vec<Out> = Vec::new();
    for (who, k, v) in plan {
        let (c, cl, doc) = &mut clients[who];
        let op = set(k, v, cl.seq);
        doc.apply(&op);
        let e = cl.edit("Color", vec![op], vec![Touch { k: k.into(), m: 'p', f: Some("color".into()), r: None, g: false }]);
        outs.extend(core.message(*c, &wire(&e), 0));
    }
    for o in outs {
        let (to, env) = match o {
            Out::All(e) => (None, e),
            Out::To(c, e) => (Some(c), e),
        };
        for (c, cl, doc) in clients.iter_mut() {
            if to.is_some_and(|t| t != *c) {
                continue;
            }
            for ev in cl.receive(&wire(&env)) {
                if let Event::Remote { ops, .. } = ev {
                    for op in &ops {
                        doc.apply(op);
                    }
                    // rebase: what is still pending goes on top of the newer commit
                    for p in &cl.pending {
                        for op in &p.ops {
                            doc.apply(op);
                        }
                    }
                }
            }
        }
    }
    // the later commit wins: a client that applied its own edit early re-applies the newer remote one over it
    let docs: Vec<&Doc> = clients.iter().map(|(_, _, d)| d).collect();
    assert_eq!(docs[0].0.get("e:1").map(String::as_str), Some("blue"));
    assert_eq!(docs[0], docs[1]);
    assert_eq!(docs[1], docs[2]);
    assert!(clients.iter().all(|(_, cl, _)| cl.pending.is_empty() && cl.seq == 5));
    assert_eq!(clients[1].1.rev("e:2"), 5);
}

#[test]
fn refusals_and_rejoin() {
    let mut core = Core::new();
    let host = core.add("Host", Role::Host);
    let ana = core.add("Ana", Role::Editor);
    let mut cl = Client::new();
    cl.receive(&Envelope::new("welcome").body(Value::map(vec![
        ("c", Value::from(ana)),
        ("role", "editor".into()),
        ("token", "t0k".into()),
    ])));
    core.lock(host, &["layer:Title".to_string()], true).unwrap();
    let e = cl.edit("Line", vec![], vec![Touch { k: "layer:Title".into(), m: 'r', f: None, r: None, g: false }]);
    let outs = core.message(ana, &wire(&e), 0);
    let Out::To(_, refuse) = &outs[0] else { panic!("{outs:?}") };
    let ev = cl.receive(&wire(refuse));
    assert!(matches!(&ev[0], Event::Refused { reason, .. } if reason == "locked"));
    assert!(cl.pending.is_empty());
    // a re-sent edit with an old counter is ignored by the core
    let e2 = cl.edit("Line", vec![], vec![Touch { k: "e:b:1".into(), m: 'c', f: None, r: None, g: false }]);
    assert_eq!(core.message(ana, &wire(&e2), 0).len(), 1);
    assert!(core.message(ana, &wire(&e2), 0).is_empty(), "decided once");
    let hello = cl.hello("cadcraft", 1, "Ana");
    let resume = hello.b.get("resume").unwrap();
    assert_eq!(resume.get("token").and_then(Value::as_str), Some("t0k"));
    assert_eq!(resume.get("c").and_then(Value::as_u64), Some(ana));
}
