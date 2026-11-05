//! Dev bench: DetailCraft open (Model::load) + build of a file, warm, best of N.
//! cargo run --release -p cc-detail --example dcbench -- <file> [runs] [open|build|both]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use cc_detail::model::Model;
use serde_json::Value;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let text = std::fs::read_to_string(&a[1]).unwrap();
    let runs: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(100);
    let only = a.get(3).cloned().unwrap_or_default();
    let text = Value::String(text);
    let mut best = [f64::INFINITY; 2];
    let mut m0 = Model::default();
    m0.load(&text).unwrap();
    for _ in 0..=runs {
        let t0 = std::time::Instant::now();
        let mut keep = None;
        if only != "build" {
            let mut m = Model::default();
            m.load(&text).unwrap();
            keep = Some(m);
        }
        let t1 = std::time::Instant::now();
        let mut kb = None;
        if only != "open" {
            let st = keep.as_ref().map_or(&m0.state, |m| &m.state);
            kb = Some(cc_detail::build::build(st).unwrap_or_default());
        }
        let t2 = std::time::Instant::now();
        drop((keep, kb));
        best[0] = best[0].min((t1 - t0).as_secs_f64() * 1e3);
        best[1] = best[1].min((t2 - t1).as_secs_f64() * 1e3);
    }
    println!("open {:.3} build {:.3} sum {:.3}", best[0], best[1], best[0] + best[1]);
}
