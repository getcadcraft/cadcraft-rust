//! Fuzz CadCraft's LISP on a drawing (the engine's drawing side: entget / entmake / ssget / (command …) / vla-):
//! any program loaded as an application must end, its edits one undo step — never a panic or a hang.
#![no_main]

use cc_cad::Doc;
use cc_cad::entity::Entity;
use cc_cad_geom::geom::Bounds;
use cc_engine::{Engine, Host, Lang};
use cc_geom::P2;
use libfuzzer_sys::fuzz_target;

#[derive(Default)]
struct Stub {
    sel: Vec<u64>,
}
impl Host for Stub {
    fn print(&mut self, _s: &str) {}
    fn prompt(&mut self, _s: &str) {}
    fn cursor(&self) -> Option<P2> {
        Some(P2::new(0.0, 0.0))
    }
    fn selection(&self) -> Vec<u64> {
        self.sel.clone()
    }
    fn set_selection(&mut self, ids: &[u64]) {
        self.sel = ids.to_vec();
    }
    fn pick_at(&self, doc: &Doc, p: P2) -> Option<u64> {
        doc.entities().find(|e| cc_cad_geom::pick::dist_to_entity(e, p, Some(&doc.blocks)) < 0.5).and_then(|e| e.id)
    }
    fn pick_window(&self, doc: &Doc, w: &Bounds, crossing: bool) -> Vec<u64> {
        doc.entities().filter(|e| cc_cad_geom::pick::in_window(e, w, crossing, Some(&doc.blocks))).filter_map(|e| e.id).collect()
    }
    fn preview(&mut self, _ents: Vec<Entity>) {}
    fn zoom_extents(&mut self) {}
    fn zoom_window(&mut self, _a: P2, _b: P2, _exact: bool) {}
}

fuzz_target!(|data: &[u8]| {
    let text = cc_lisp::reader::decode_source(data);
    let mut doc = Doc::new();
    doc.load(&serde_json::json!({
        "layers": [{ "name": "0" }, { "name": "A", "locked": true }],
        "blocks": [{ "name": "B", "base": { "x": 0, "y": 0 }, "entities": [{ "type": "line", "x1": 0, "y1": 0, "x2": 1, "y2": 0 }], "attdefs": [{ "tag": "T", "x": 0, "y": 0, "h": 1 }] }],
        "entities": [
            { "id": 1, "type": "line", "layer": "0", "x1": 0, "y1": 0, "x2": 10, "y2": 0 },
            { "id": 2, "type": "circle", "layer": "A", "cx": 5, "cy": 5, "r": 2 },
            { "id": 3, "type": "insert", "layer": "0", "block": "B", "x": 1, "y": 1, "attrs": { "T": "v" } },
            { "id": 4, "type": "text", "layer": "0", "x": 0, "y": 9, "h": 1, "s": "t" }
        ]
    }));
    let mut eng = Engine::new(doc, Box::new(Stub::default()), Lang::default());
    // 20 000 steps: a loop that adds to the drawing and walks it (ssget "X", (command …) with its undo step) costs
    // O(n) a step, so 200 000 ran ~20 min under the fuzzer (seed while-that-grows-the-drawing.lsp); the window's
    // budget is 0.5's, and Esc stops a long routine there
    eng.set_lisp_limits(cc_lisp::Limits { steps: Some(20_000), frames: 100_000, string: 1 << 20, array: 1 << 16 });
    eng.lisp_load(vec![cc_engine::lisp::AppFile { name: "f.lsp".into(), text, kind: "lsp".into() }], "SCRIPT");
    // a routine waiting for the person: answer a few prompts, then Esc
    for answer in ["1,1", "", "2", "Y"] {
        if !eng.busy() {
            break;
        }
        eng.text(answer);
    }
    while eng.busy() {
        if !eng.escape() {
            break;
        }
    }
});
