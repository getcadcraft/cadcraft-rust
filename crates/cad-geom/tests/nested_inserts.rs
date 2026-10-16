//! Regression (nightly fuzz, cad_dxf_read OOM 2026-10-16): blocks inserted inside blocks multiply. Five levels of 30
//! inserts each is 24 million pieces for one top-level insert; the outline stops at `MAX_PRIMS` instead of growing
//! one vector to gigabytes.

use cc_cad::Doc;
use cc_cad_geom::geom::{MAX_PRIMS, prims};
use serde_json::json;

#[test]
fn nested_inserts_stop_at_the_budget() {
    let mut blocks = vec![json!({ "name": "B0", "base": { "x": 0, "y": 0 }, "entities": [
        { "type": "line", "x1": 0, "y1": 0, "x2": 1, "y2": 0 },
        { "type": "line", "x1": 0, "y1": 0, "x2": 0, "y2": 1 } ] })];
    for lvl in 1..=5 {
        let ents: Vec<_> = (0..30).map(|i| json!({ "type": "insert", "block": format!("B{}", lvl - 1), "x": i, "y": 0 })).collect();
        blocks.push(json!({ "name": format!("B{lvl}"), "base": { "x": 0, "y": 0 }, "entities": ents }));
    }
    let mut d = Doc::new();
    d.load(&json!({ "blocks": blocks, "entities": [{ "type": "insert", "block": "B5", "x": 0, "y": 0 }] }));
    let e = d.model().next().unwrap().clone();
    let t = std::time::Instant::now();
    let ps = prims(&e, Some(&d.blocks));
    assert_eq!(ps.len(), MAX_PRIMS);
    assert!(t.elapsed().as_secs() < 20, "bounded work: {:?}", t.elapsed());
    // a small nest is untouched: 30 inserts of 2 lines
    let mut d2 = Doc::new();
    d2.load(&json!({ "blocks": blocks[..2].to_vec(), "entities": [{ "type": "insert", "block": "B1", "x": 0, "y": 0 }] }));
    let e2 = d2.model().next().unwrap().clone();
    assert_eq!(prims(&e2, Some(&d2.blocks)).len(), 60);
}

#[test]
fn a_walk_of_nested_inserts_ends() {
    use cc_cad_geom::walk::{Opts, WalkOpts, WalkOut, walk};
    struct Count(usize);
    impl WalkOut for Count {
        fn stroke(&mut self, _c: &str, p: Vec<cc_cad_geom::geom::Prim>, _lw: f64, _o: Option<Opts>) {
            self.0 += p.len();
        }
        fn fill(&mut self, _c: &str, _l: Vec<Vec<f64>>, _s: bool, _o: Option<Opts>) {}
        fn text(&mut self, _c: &str, _e: &cc_cad::Entity, _o: Option<Opts>) {}
        fn point(&mut self, _c: &str, _x: f64, _y: f64) {}
    }
    // 7 levels of 30: 21.9 billion lines without the visit budget
    let mut blocks =
        vec![json!({ "name": "B0", "base": { "x": 0, "y": 0 }, "entities": [{ "type": "line", "x1": 0, "y1": 0, "x2": 1, "y2": 0 }] })];
    for lvl in 1..=7 {
        let ents: Vec<_> = (0..30).map(|i| json!({ "type": "insert", "block": format!("B{}", lvl - 1), "x": i, "y": 0 })).collect();
        blocks.push(json!({ "name": format!("B{lvl}"), "base": { "x": 0, "y": 0 }, "entities": ents }));
    }
    let mut d = Doc::new();
    d.load(&json!({ "blocks": blocks, "entities": [{ "type": "insert", "block": "B7", "x": 0, "y": 0 }] }));
    let ents: Vec<cc_cad::Entity> = d.model().cloned().collect();
    let mut c = Count(0);
    let t = std::time::Instant::now();
    walk(&d, &ents, &mut c, &WalkOpts::default());
    assert!(c.0 > 0 && c.0 < cc_cad_geom::walk::MAX_WALK_VISITS, "{}", c.0);
    assert!(t.elapsed().as_secs() < 60, "bounded work: {:?}", t.elapsed());
}
