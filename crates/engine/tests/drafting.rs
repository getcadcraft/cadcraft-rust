//! The drafting commands past the M4 set (M4.10), driven as the 0.5 `bin/test-cadcraft.mjs` drives them: a stub
//! host, typed lines, clicks and windows, with the 0.5 English catalog. Cases from its sections "Hatch", "Fillet and
//! chamfer", "STRETCH and ARRAY", "Dimensions", "Blocks", "Layer tools", "BREAK and LENGTHEN". Parity with the frozen
//! JS over many generated inputs is `forks/tools/kdiff.mjs` (`cadcraft-cli cmds`); these are the regression tests that
//! live in the repo.

use std::cell::RefCell;
use std::f64::consts::PI;
use std::rc::Rc;

use cc_cad::Doc;
use cc_cad::entity::{Entity, Geo};
use cc_cad_geom::geom::Bounds;
use cc_cad_geom::hatch::boundary_at;
use cc_cad_geom::pick::{dist_to_entity, in_window};
use cc_engine::{Engine, Host, Lang};
use cc_geom::P2;
use serde_json::{Value, json};

static EN: &str = include_str!("../../../assets/lang/cadcraft/en.json");
fn lang() -> Lang {
    Lang::new(&serde_json::from_str::<Value>(EN).unwrap())
}

#[derive(Default)]
struct Log {
    printed: Vec<String>,
    cursor: Option<P2>,
    shift: bool,
    selection: Vec<u64>,
}
struct Stub(Rc<RefCell<Log>>);
impl Host for Stub {
    fn print(&mut self, s: &str) {
        self.0.borrow_mut().printed.push(s.into());
    }
    fn prompt(&mut self, _s: &str) {}
    fn cursor(&self) -> Option<P2> {
        self.0.borrow().cursor
    }
    fn shift(&self) -> bool {
        self.0.borrow().shift
    }
    fn selection(&self) -> Vec<u64> {
        self.0.borrow().selection.clone()
    }
    fn set_selection(&mut self, ids: &[u64]) {
        self.0.borrow_mut().selection = ids.to_vec();
    }
    fn pick_at(&self, doc: &Doc, p: P2) -> Option<u64> {
        let mut best = None;
        let mut bd = 0.5;
        for e in doc.entities() {
            let d = dist_to_entity(e, p, Some(&doc.blocks));
            if d < bd {
                bd = d;
                best = e.id;
            }
        }
        best
    }
    fn pick_window(&self, doc: &Doc, w: &Bounds, crossing: bool) -> Vec<u64> {
        doc.entities().filter(|e| in_window(e, w, crossing, Some(&doc.blocks))).filter_map(|e| e.id).collect()
    }
    fn preview(&mut self, _ents: &[Entity]) {}
    fn zoom_extents(&mut self) {}
    fn zoom_window(&mut self, _a: P2, _b: P2, _exact: bool) {}
}

struct H {
    eng: Engine,
    log: Rc<RefCell<Log>>,
}
impl H {
    fn new() -> H {
        let log = Rc::new(RefCell::new(Log { cursor: Some(P2::new(0.0, 0.0)), ..Log::default() }));
        let eng = Engine::new(Doc::new(), Box::new(Stub(Rc::clone(&log))), lang());
        H { eng, log }
    }
    fn ty(&mut self, lines: &[&str]) {
        for l in lines {
            self.eng.text(l);
        }
    }
    fn click(&mut self, x: f64, y: f64) {
        self.eng.click(P2::new(x, y), None, P2::new(x, y));
    }
    fn all(&self) -> Vec<Entity> {
        self.eng.doc().all()
    }
    fn of(&self, t: &str) -> Vec<Entity> {
        self.all().into_iter().filter(|e| e.typ() == t).collect()
    }
    fn said(&self, s: &str) -> bool {
        self.log.borrow().printed.iter().any(|p| p == s)
    }
    fn printed_has(&self, part: &str) -> bool {
        self.log.borrow().printed.iter().any(|p| p.contains(part))
    }
    fn undo_labels(&self) -> Vec<String> {
        self.eng.doc().undo_stack.iter().map(|s| s.label.clone()).collect()
    }
    fn sel(&self, ids: &[u64]) {
        self.log.borrow_mut().selection = ids.to_vec();
    }
    fn t(&self, k: &str, vars: &[(&str, String)]) -> String {
        lang().t(k, vars)
    }
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}
fn ent(v: Value) -> Entity {
    Entity::from_value(&v)
}
fn loop_area(l: &[f64]) -> f64 {
    let n = l.len() / 2;
    let mut a = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        a += l[2 * i] * l[2 * j + 1] - l[2 * j] * l[2 * i + 1];
    }
    (a / 2.0).abs()
}

// ---------------------------------------------------------------------------------------------------- boundaries

fn hash_sign() -> Vec<Entity> {
    [(-2.0, 0.0, 12.0, 0.0), (-2.0, 10.0, 12.0, 10.0), (0.0, -2.0, 0.0, 12.0), (10.0, -2.0, 10.0, 12.0)]
        .iter()
        .map(|(a, b, c, d)| ent(json!({"type": "line", "layer": "0", "x1": a, "y1": b, "x2": c, "y2": d})))
        .collect()
}

#[test]
fn boundary_four_crossing_lines_close_a_room_with_a_circle_island() {
    let lines = hash_sign();
    let refs: Vec<&Entity> = lines.iter().collect();
    let b1 = boundary_at(&refs, P2::new(5.0, 5.0), None, None).unwrap();
    assert_eq!(b1.len(), 1);
    assert!(near(loop_area(&b1[0]), 100.0, 1e-6), "the 10 × 10 room");
    let circle = ent(json!({"type": "circle", "layer": "0", "cx": 5, "cy": 5, "r": 2}));
    let mut with: Vec<&Entity> = lines.iter().collect();
    with.push(&circle);
    let b2 = boundary_at(&with, P2::new(1.0, 1.0), None, None).unwrap();
    assert_eq!(b2.len(), 2, "the circle inside is an island");
    assert!(near(loop_area(&b2[1]), PI * 4.0, 0.01));
    let b3 = boundary_at(&with, P2::new(5.0, 5.0), None, None).unwrap();
    assert!(b3.len() == 1 && near(loop_area(&b3[0]), PI * 4.0, 0.01), "inside the circle: the circle itself");
    let three: Vec<&Entity> = lines.iter().take(3).collect();
    assert_eq!(boundary_at(&three, P2::new(5.0, 5.0), None, None).unwrap_err().code(), "open");
    assert_eq!(boundary_at(&[], P2::new(0.0, 0.0), None, None).unwrap_err().code(), "none");
    let spur = ent(json!({"type": "line", "layer": "0", "x1": 5, "y1": 0, "x2": 5, "y2": 4}));
    let mut sp: Vec<&Entity> = lines.iter().collect();
    sp.push(&spur);
    let b5 = boundary_at(&sp, P2::new(2.0, 5.0), None, None).unwrap();
    assert!(b5.len() == 1 && near(loop_area(&b5[0]), 100.0, 1e-6), "a line poking into the room does not cut it");
}

#[test]
fn boundary_with_an_arc_side_and_hostile_input() {
    let room = [
        json!({"type": "line", "layer": "0", "x1": 0, "y1": 0, "x2": 10, "y2": 0}),
        json!({"type": "line", "layer": "0", "x1": 10, "y1": 0, "x2": 10, "y2": 10}),
        json!({"type": "arc", "layer": "0", "cx": 5, "cy": 10, "r": 5, "a0": 0, "a1": PI}),
        json!({"type": "line", "layer": "0", "x1": 0, "y1": 10, "x2": 0, "y2": 0}),
    ]
    .map(ent);
    let refs: Vec<&Entity> = room.iter().collect();
    let b = boundary_at(&refs, P2::new(5.0, 12.0), None, None).unwrap();
    assert!(near(loop_area(&b[0]), 100.0 + PI * 12.5, 0.05), "an arc side follows the arc");
    // NaN / infinite coordinates and a degenerate everything never panic
    let bad = [
        json!({"type": "line", "layer": "0", "x1": 0, "y1": 0, "x2": 1e308, "y2": -1e308}),
        json!({"type": "line", "layer": "0", "x1": 0, "y1": 0, "x2": 0, "y2": 0}),
        json!({"type": "circle", "layer": "0", "cx": 0, "cy": 0, "r": 1e-300}),
    ]
    .map(ent);
    let refs: Vec<&Entity> = bad.iter().collect();
    let _ = boundary_at(&refs, P2::new(f64::NAN, 0.0), None, None);
    let _ = boundary_at(&refs, P2::new(0.5, 0.5), None, None);
}

// ---------------------------------------------------------------------------------------------------- HATCH

#[test]
fn hatch_pattern_scale_pick_and_island_in_one_undo_step() {
    let mut h = H::new();
    h.ty(&["L", "-2,0", "12,0", "", "L", "-2,10", "12,10", "", "L", "0,-2", "0,12", "", "L", "10,-2", "10,12", ""]);
    h.ty(&["C", "5,5", "2"]);
    let n0 = h.eng.doc().undo_stack.len();
    h.ty(&["H", "P", "ANSI37", "SC", "0.5"]);
    h.click(1.0, 1.0);
    h.ty(&[""]);
    let hh = h.of("hatch").pop().expect("a hatch");
    let v = hh.to_value();
    assert_eq!(v["pattern"], "ANSI37");
    assert_eq!(v["pscale"], 0.5);
    assert_eq!(v["loops"].as_array().unwrap().len(), 2, "the circle is an island");
    assert_eq!(h.eng.doc().undo_stack.len(), n0 + 1);
    h.ty(&["H", "P", "SOLID", "S", "L", "", ""]);
    assert!(h.said(&h.t("cmd.hatch.notclosed", &[("n", "1".into())])));
    h.ty(&["H", "20,20", ""]);
    assert!(h.said(&h.t("cmd.hatch.err.open", &[])));
    h.ty(&["H", "P", "?", ""]);
    assert!(h.printed_has("ANSI31 — "));
}

// ---------------------------------------------------------------------------------------------------- FILLET / CHAMFER

fn line_of(e: &Entity) -> (f64, f64, f64, f64) {
    if let Geo::Line(l) = &e.geo { (l.x1, l.y1, l.x2, l.y2) } else { panic!("not a line") }
}

#[test]
fn fillet_two_lines_radius_two_is_one_step() {
    let mut h = H::new();
    h.ty(&["L", "0,0", "10,0", "", "L", "0,0", "0,10", ""]);
    h.ty(&["F", "R", "2"]);
    h.click(8.0, 0.0);
    h.click(0.0, 8.0);
    let ls = h.of("line");
    let arc = h.of("arc").pop().expect("an arc");
    let (ax1, ay1, _, _) = line_of(&ls[0]);
    let (bx1, by1, _, _) = line_of(&ls[1]);
    assert!(near(ax1, 2.0, 1e-9) && near(ay1, 0.0, 1e-9) && near(bx1, 0.0, 1e-9) && near(by1, 2.0, 1e-9));
    let Geo::Arc(a) = &arc.geo else { panic!() };
    assert!(near(a.cx, 2.0, 1e-9) && near(a.cy, 2.0, 1e-9) && near(a.r, 2.0, 1e-9));
    assert_eq!(h.undo_labels().last().map(String::as_str), Some("FILLET"));
    assert_eq!(h.eng.doc().undo_stack.len(), 3);
}

#[test]
fn fillet_shift_makes_a_sharp_corner_and_polyline_fillets_every_corner() {
    let mut h = H::new();
    h.ty(&["L", "0,0", "10,0", "", "L", "0,0", "0,10", ""]);
    h.ty(&["F", "R", "3"]);
    h.click(8.0, 0.0);
    h.log.borrow_mut().shift = true;
    h.click(0.0, 8.0);
    h.log.borrow_mut().shift = false;
    assert_eq!(h.eng.doc().count(), 2);
    assert!(h.of("arc").is_empty());
    let mut p = H::new();
    p.ty(&["REC", "0,0", "20,10"]);
    p.ty(&["F", "R", "2", "P"]);
    p.click(10.0, 0.0);
    let pl = p.of("poly").pop().unwrap();
    let Geo::Poly(poly) = &pl.geo else { panic!() };
    assert_eq!(poly.pts.len(), 8);
    assert_eq!(poly.pts.iter().filter(|v| near(v.bulge(), (PI / 8.0).tan(), 1e-9)).count(), 4);
}

#[test]
fn chamfer_two_lines_by_distance() {
    let mut h = H::new();
    h.ty(&["L", "0,0", "10,0", "", "L", "0,0", "0,10", ""]);
    h.ty(&["CHA", "D", "2", "3"]);
    h.click(8.0, 0.0);
    h.click(0.0, 8.0);
    let ls = h.of("line");
    assert_eq!(ls.len(), 3, "the two lines and the bevel");
    assert_eq!(h.undo_labels().last().map(String::as_str), Some("CHAMFER"));
}

// ---------------------------------------------------------------------------------------------------- STRETCH / ARRAY

#[test]
fn stretch_with_a_crossing_window_widens_the_rectangle() {
    let mut h = H::new();
    h.ty(&["REC", "0,0", "10,10"]);
    h.ty(&["L", "10,5", "20,5", ""]);
    h.ty(&["S"]);
    h.eng.window(Bounds { min_x: 8.0, min_y: -1.0, max_x: 12.0, max_y: 11.0 }, true);
    h.ty(&["", "0,0", "5,0"]);
    let all = h.all();
    let Geo::Poly(r) = &all[0].geo else { panic!() };
    assert!(near(r.pts[1].x, 15.0, 1e-9) && near(r.pts[2].x, 15.0, 1e-9) && near(r.pts[0].x, 0.0, 1e-9));
    let (x1, _, x2, _) = line_of(&all[1]);
    assert!(near(x1, 15.0, 1e-9) && near(x2, 20.0, 1e-9));
    assert_eq!(h.undo_labels().last().map(String::as_str), Some("STRETCH"));
    h.ty(&["S", "C", "14,-1", "16,11", "", "D", "-5,0"]);
    let Geo::Poly(r) = &h.all()[0].geo else { panic!() };
    assert!(near(r.pts[1].x, 10.0, 1e-9), "C typed and Displacement");
}

#[test]
fn array_rectangular_polar_and_limits() {
    let mut h = H::new();
    h.ty(&["C", "0,0", "1"]);
    h.ty(&["AR", "L", "", "R", "COU", "3", "2", "S", "5", "-4", ""]);
    let cs = h.of("circle");
    assert_eq!(cs.len(), 6);
    assert!(cs.iter().any(|e| matches!(&e.geo, Geo::Circle(c) if near(c.cx, 10.0, 1e-9) && near(c.cy, -4.0, 1e-9))));
    assert!(h.said(&h.t("cmd.array.done", &[("n", "5".into())])));
    h.ty(&["U"]);
    h.ty(&["ARRAYPOLAR", "L", "", "0,10", "I", "4", ""]);
    let p4 = h.of("circle");
    assert_eq!(p4.len(), 4);
    assert!(p4.iter().any(|e| matches!(&e.geo, Geo::Circle(c) if near(c.cx, 0.0, 1e-9) && near(c.cy, 20.0, 1e-9))));
    h.ty(&["U"]);
    h.ty(&["AR", "L", "", "R", "COU", "1", "1", ""]);
    assert!(h.said(&h.t("cmd.array.one", &[])));
    h.ty(&["AR", "L", "", "R", "COU", "1000", "1000", ""]);
    assert!(h.said(&h.t("cmd.array.toomany", &[])));
}

// ---------------------------------------------------------------------------------------------------- dimensions

fn dim_measure(e: &Entity) -> f64 {
    cc_cad_geom::dims::dim_measure(e)
}

#[test]
fn dimensions_linear_aligned_continue_angular_radius_scale() {
    let mut h = H::new();
    h.ty(&["DLI", "0,0", "100,40", "50,-15"]);
    h.ty(&["DLI", "0,0", "100,40", "120,20"]);
    let d = h.of("dim");
    assert!(
        near(dim_measure(&d[0]), 100.0, 1e-9) && near(dim_measure(&d[1]), 40.0, 1e-9),
        "horizontal or vertical from where the line goes"
    );
    h.ty(&["DLI", "0,0", "100,40", "R", "45", "50,50"]);
    assert!(near(dim_measure(&h.of("dim")[2]), 140.0 / 2f64.sqrt(), 1e-6), "Rotated");
    h.ty(&["DAL", "0,0", "30,40", "T", "about <>", "0,10"]);
    let al = h.of("dim")[3].clone();
    assert_eq!(cc_cad_geom::dims::dim_text(&al), "about 50");
    h.ty(&["DCO", "150,0", "200,0", ""]);
    assert_eq!(h.of("dim").len(), 6);
    h.ty(&["L", "0,100", "50,100", "", "L", "0,100", "0,150", ""]);
    h.ty(&["DAN"]);
    h.click(25.0, 100.0);
    h.click(0.0, 125.0);
    h.ty(&["10,110"]);
    assert!(near(dim_measure(&h.of("dim").pop().unwrap()), PI / 2.0, 1e-9));
    h.ty(&["C", "200,200", "10"]);
    h.ty(&["DRA"]);
    h.click(210.0, 200.0);
    h.ty(&["230,220"]);
    assert_eq!(cc_cad_geom::dims::dim_text(&h.of("dim").pop().unwrap()), "R10");
    h.ty(&["DIMSCALE", "50"]);
    assert_eq!(h.eng.doc().current.dim.get("scale"), Some(&json!(50)));
    assert_eq!(h.eng.doc().undo_stack.len(), 10, "each command one undo step");
}

#[test]
fn dimbaseline_stacks_away_from_the_object() {
    let mut h = H::new();
    h.ty(&["DLI", "0,0", "10,0", "5,-5"]);
    h.ty(&["DBA", "20,0", "30,0", ""]);
    let d = h.of("dim");
    let loc = |e: &Entity| if let Geo::Dim(x) = &e.geo { x.loc.unwrap() } else { panic!() };
    assert_eq!(d.len(), 3);
    assert!(loc(&d[1]).y < loc(&d[0]).y && loc(&d[2]).y < loc(&d[1]).y);
}

// ---------------------------------------------------------------------------------------------------- blocks

#[test]
fn block_insert_purge_and_the_block_editor() {
    let mut h = H::new();
    h.ty(&["C", "0,0", "5"]);
    h.ty(&["L", "-5,0", "5,0", ""]);
    let ids: Vec<u64> = h.all().iter().filter_map(|e| e.id).collect();
    h.sel(&ids);
    h.ty(&["B", "Bolt M10", "0,0"]);
    assert!(h.eng.doc().blocks.get(&"Bolt M10".to_string()).is_some());
    assert_eq!(h.eng.doc().count(), 1);
    assert_eq!(h.eng.doc().undo_stack.len(), 3, "BLOCK is one undo step");
    h.ty(&["I", "Bolt M10", "S", "2", "R", "90", "100,0"]);
    let ins = h.of("insert").pop().unwrap();
    let Geo::Insert(i) = &ins.geo else { panic!() };
    assert!(i.sx == Some(2.0) && near(i.rot.unwrap(), PI / 2.0, 1e-9) && near(i.x, 100.0, 1e-9));
    // a block cannot contain itself
    let first = h.all()[0].id.unwrap();
    h.sel(&[first]);
    h.ty(&["B", "Bolt M10", "Y", "0,0"]);
    assert!(h.eng.running().is_none());
    assert_eq!(h.eng.doc().blocks.get(&"Bolt M10".to_string()).unwrap().entities.len(), 2);
    // the block editor: add a line inside, save, the definition has three objects in one BEDIT step
    let before = h.eng.doc().undo_stack.len();
    h.ty(&["BE", "Bolt M10"]);
    assert_eq!(h.eng.doc().count(), 2, "the editor shows the block's objects");
    h.ty(&["L", "0,0", "0,9", ""]);
    h.ty(&["BCLOSE", "S"]);
    assert_eq!(h.eng.doc().blocks.get(&"Bolt M10".to_string()).unwrap().entities.len(), 3);
    assert_eq!(h.eng.doc().undo_stack.len(), before + 1);
    assert!(h.eng.doc().in_model());
    // and a discarded edit leaves the definition as it was
    h.ty(&["BE", "Bolt M10", "E", "ALL", "", "BCLOSE", "D"]);
    assert_eq!(h.eng.doc().blocks.get(&"Bolt M10".to_string()).unwrap().entities.len(), 3);
    assert_eq!(h.eng.doc().undo_stack.len(), before + 1);
    h.ty(&["PU"]);
    assert!(h.eng.doc().blocks.get(&"Bolt M10".to_string()).is_some(), "PURGE keeps a block in use");
}

// ---------------------------------------------------------------------------------------------------- layers

#[test]
fn layer_tools_iso_uniso_off_and_merge() {
    let mut h = H::new();
    let mut d = Doc::new();
    d.load(&json!({
        "layers": [{"name": "0"}, {"name": "A"}, {"name": "B"}],
        "entities": [
            {"id": 1, "type": "line", "layer": "A", "x1": 0, "y1": 0, "x2": 10, "y2": 0},
            {"id": 2, "type": "line", "layer": "B", "x1": 0, "y1": 5, "x2": 10, "y2": 5}
        ]
    }));
    *h.eng.doc_mut() = d;
    h.ty(&["LAYISO"]);
    h.click(5.0, 0.0);
    h.ty(&[""]);
    let on = |h: &H, n: &str| h.eng.doc().layers.get(&n.to_string()).unwrap().on;
    assert!(on(&h, "A") && !on(&h, "B") && !on(&h, "0"));
    h.ty(&["LAYUNISO"]);
    assert!(on(&h, "B") && on(&h, "0"));
    h.ty(&["LAYMRG"]);
    h.click(5.0, 5.0);
    h.ty(&["", "N", "A", "Y"]);
    assert!(h.eng.doc().layers.get(&"B".to_string()).is_none());
    assert!(h.all().iter().all(|e| e.layer.as_deref() == Some("A")));
}

// ---------------------------------------------------------------------------------------------------- BREAK / LENGTHEN

#[test]
fn break_and_lengthen_a_line() {
    let mut h = H::new();
    h.ty(&["L", "0,0", "10,0", ""]);
    h.ty(&["BR"]);
    h.click(3.0, 0.0);
    h.ty(&["6,0"]);
    assert_eq!(h.of("line").len(), 2);
    h.ty(&["LEN", "DE", "2"]);
    h.click(9.0, 0.0);
    h.ty(&[""]);
    let ls = h.of("line");
    assert!(
        ls.iter().any(|e| {
            let (_, _, x2, _) = line_of(e);
            near(x2, 12.0, 1e-9)
        }),
        "the far end moved 2 out"
    );
}

#[test]
fn id_sets_keep_order_and_stay_fast_on_big_selections() {
    let mut s = cc_engine::IdSet::from_vec(vec![3, 1, 3, 2]);
    assert_eq!(s.0, [3, 1, 2]);
    assert!(s.delete(1) && !s.delete(1) && s.has(2) && !s.has(1));
    assert!(s.add(1) && !s.add(1));
    assert_eq!(s.0, [3, 2, 1]);
    // ALL on 20,000 objects: was quadratic (seconds), now one pass
    let mut h = H::new();
    let ents: Vec<Value> =
        (0..20_000).map(|i| json!({"id": i + 1, "type": "line", "layer": "0", "x1": i, "y1": 0, "x2": i, "y2": 1})).collect();
    let mut d = Doc::new();
    d.load(&json!({ "layers": [{"name": "0"}], "entities": ents }));
    *h.eng.doc_mut() = d;
    let t0 = std::time::Instant::now();
    h.ty(&["E", "ALL", ""]);
    assert_eq!(h.eng.doc().count(), 0);
    assert!(t0.elapsed().as_secs_f64() < 20.0);
}
