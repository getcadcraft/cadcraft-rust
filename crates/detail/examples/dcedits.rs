//! Dev bench: an edit (a member moved), its undo and redo on a file, the model before S.16 (one JSON value, feature
//! `reference`) against the state of records, best of N each.
//! cargo run --release -p cc-detail --features reference --example dcedits -- <file> [runs]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use serde_json::{Value, json};
use std::time::Instant;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let text = Value::String(std::fs::read_to_string(&a[1]).unwrap());
    let runs: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(50);
    let (old, _) = cc_detail::reference::read_model(&text).unwrap();
    let mut o = cc_detail::reference::Model::new(old);
    let mut n = cc_detail::model::Model::default();
    n.load(&text).unwrap();
    let id = n.state.list("members")[0].get("id").unwrap().to_value();
    let mut best = [[f64::INFINITY; 3]; 2];
    for r in 0..runs {
        let patch = json!({ "start": [r as f64 * 10.0 + 12_345.5, 7.25, 0] });
        let t0 = Instant::now();
        o.change("e", |tx| tx.call("updateMember", &[id.clone(), patch.clone()])).unwrap();
        let t1 = Instant::now();
        o.undo();
        let t2 = Instant::now();
        o.redo();
        let t3 = Instant::now();
        n.change("e", |tx| tx.call("updateMember", &[id.clone(), patch.clone()])).unwrap();
        let t4 = Instant::now();
        n.undo();
        let t5 = Instant::now();
        n.redo();
        let t6 = Instant::now();
        for (i, d) in [t1 - t0, t2 - t1, t3 - t2].into_iter().enumerate() {
            best[0][i] = best[0][i].min(d.as_secs_f64() * 1e3);
        }
        for (i, d) in [t4 - t3, t5 - t4, t6 - t5].into_iter().enumerate() {
            best[1][i] = best[1][i].min(d.as_secs_f64() * 1e3);
        }
    }
    assert_eq!(cc_detail::js::stringify(&o.state), n.state.stringify());
    println!(
        "edit / undo / redo ms  before {:.3} / {:.3} / {:.3}  now {:.3} / {:.3} / {:.3}",
        best[0][0], best[0][1], best[0][2], best[1][0], best[1][1], best[1][2]
    );
}
