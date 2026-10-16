//! Fuzz the drafting document loader (doc.load's input is file data) and the geometry walk over what it keeps:
//! any JSON must load, snapshot and draw, never panic, hang or OOM.
#![no_main]

use cc_cad::Doc;
use cc_cad::entity::Entity;
use cc_cad_geom::geom::Prim;
use cc_cad_geom::walk::{Opts, WalkOpts, WalkOut, walk};
use libfuzzer_sys::fuzz_target;

struct Count(usize);

impl WalkOut for Count {
    fn stroke(&mut self, _c: &str, prims: Vec<Prim>, _lw: f64, _o: Option<Opts>) {
        self.0 = self.0.saturating_add(prims.len());
    }
    fn fill(&mut self, _c: &str, loops: Vec<Vec<f64>>, _s: bool, _o: Option<Opts>) {
        self.0 = self.0.saturating_add(loops.len());
    }
    fn text(&mut self, _c: &str, _e: &Entity, _o: Option<Opts>) {}
    fn point(&mut self, _c: &str, _x: f64, _y: f64) {}
}

fuzz_target!(|data: &[u8]| {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(data) else { return };
    let mut doc = Doc::new();
    doc.load(&v);
    let _ = doc.snapshot();
    let ents: Vec<Entity> = doc.model().cloned().collect();
    for e in &ents {
        let _ = cc_cad_geom::tx::grips(e);
        let _ = cc_cad_geom::corners::explode_entity(e, Some(&doc.blocks));
    }
    walk(&doc, &ents, &mut Count(0), &WalkOpts::default());
});
