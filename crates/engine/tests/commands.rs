//! The command-line cases of the 0.5 `bin/test-cadcraft.mjs` (sections "Command line", "TRIM and EXTEND", "Selection
//! options", "ZOOM Previous / Scale / Center", "Tangent object snap, deferred", "Tangent circles", the TRIM of
//! construction lines and typed points in a UCS) for the commands ported to cc-engine, driven the same way: a stub
//! host, typed lines, clicks and windows. Parts that need commands not ported yet (FILLET, EXPLODE, SELECT, NAMEDVIEW,
//! XLINE, UCS) are left out or replaced by a document loaded with the same objects.

use std::cell::RefCell;
use std::f64::consts::{PI, SQRT_2};
use std::rc::Rc;

use cc_cad::Doc;
use cc_cad::entity::{Entity, Geo};
use cc_cad_geom::geom::{Bounds, prim_point, prims};
use cc_cad_geom::pick::{Circ, dist_to_entity, in_window};
use cc_engine::prompt::Kind;
use cc_engine::tancircle::{Curve, circle_ttr, circle_ttt};
use cc_engine::{Engine, Host, Lang, Tan};
use cc_geom::P2;
use serde_json::json;

#[derive(Default)]
struct Log {
    printed: Vec<String>,
    last_prompt: String,
    cursor: Option<P2>,
    shift: bool,
    selection: Vec<u64>,
    calls: Vec<String>,
    prev_calls: u32,
    view: Option<(f64, f64, f64, f64)>,
}

struct Stub(Rc<RefCell<Log>>);

impl Host for Stub {
    fn print(&mut self, s: &str) {
        self.0.borrow_mut().printed.push(s.into());
    }
    fn prompt(&mut self, s: &str) {
        self.0.borrow_mut().last_prompt = s.into();
    }
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
    fn zoom_extents(&mut self) {
        self.0.borrow_mut().calls.push("extents".into());
    }
    fn zoom_window(&mut self, a: P2, b: P2, exact: bool) {
        let mut l = self.0.borrow_mut();
        l.calls.push(format!("win {} {} {} {} {exact}", a.x, a.y, b.x, b.y));
        l.view = Some(((a.x + b.x) / 2.0, (a.y + b.y) / 2.0, (b.x - a.x).abs(), (b.y - a.y).abs()));
    }
    fn zoom_previous(&mut self) -> bool {
        let mut l = self.0.borrow_mut();
        l.calls.push("prev".into());
        l.prev_calls += 1;
        l.prev_calls < 2
    }
    fn zoom_by(&mut self, f: f64) {
        self.0.borrow_mut().calls.push(format!("by {f}"));
    }
    fn view_rect(&self) -> Option<(f64, f64)> {
        self.0.borrow().view.map(|v| (v.2, v.3))
    }
}

/// The few catalog strings the checks read (copied from the 0.5 lang/en.json).
fn lang() -> Lang {
    Lang::new(&json!({
        "cmd.echo": "Command: {name}",
        "cmd.unknown": "Unknown command \"{name}\". Type HELP for the list.",
        "cmd.trim.pick": "Select object to trim or shift-select to extend",
        "opt.cuttingedges": "cuTting edges",
        "cmd.trim.done": { "one": "Trimmed {n} object.", "other": "Trimmed {n} objects." },
        "cmd.trim.settings": "Current settings: {mode} mode, edges: {edge}",
        "opt.quick": "Quick",
        "opt.noextend": "No extend",
        "opt.extend": "Extend",
        "opt.standard": "Standard",
        "cmd.trim.erased": { "one": "Nothing crossed {n} object, so it was erased (Quick mode; mOde, Standard keeps such objects).", "other": "Nothing crossed {n} objects, so they were erased (Quick mode; mOde, Standard keeps such objects)." },
        "cmd.trim.edges.n": { "one": "{n} edge chosen.", "other": "{n} edges chosen." },
        "cmd.trim.nothing": "Nothing to trim there: no chosen edge crosses that object.",
        "cmd.extend.pick": "Select object to extend or shift-select to trim",
        "sel.help": "Click objects or drag a window, then press Enter.",
        "sel.remove.prompt": "Remove objects",
        "sel.removed": { "one": "{n} removed", "other": "{n} removed" },
        "sel.previous.none": "There is no previous selection yet.",
        "cmd.zoom.noprev": "There is no previous view yet.",
        "cmd.circle.no_circle": "Circle does not exist.",
        "cmd.sim.select": "Select objects",
        "opt.undo": "Undo",
        "opt.close": "Close",
        "sel.opt.fence": "Fence",
        "sel.opt.crossing": "Crossing",
        "sel.opt.window": "Window",
        "sel.opt.previous": "Previous",
        "sel.opt.remove": "Remove",
        "sel.opt.add": "Add"
    }))
}

struct H {
    eng: Engine,
    log: Rc<RefCell<Log>>,
}

impl H {
    fn new() -> H {
        H::with(Doc::new())
    }
    fn load(v: serde_json::Value) -> H {
        let mut d = Doc::new();
        d.load(&v);
        H::with(d)
    }
    fn with(doc: Doc) -> H {
        let log = Rc::new(RefCell::new(Log { cursor: Some(P2::new(0.0, 0.0)), ..Log::default() }));
        let eng = Engine::new(doc, Box::new(Stub(Rc::clone(&log))), lang());
        H { eng, log }
    }
    fn ty(&mut self, lines: &[&str]) {
        for l in lines {
            self.eng.text(l);
        }
    }
    fn click(&mut self, x: f64, y: f64) -> bool {
        self.eng.click(P2::new(x, y), None, P2::new(x, y))
    }
    fn count(&self) -> usize {
        self.eng.doc().count()
    }
    fn all(&self) -> Vec<Entity> {
        self.eng.doc().all()
    }
    fn printed(&self) -> Vec<String> {
        self.log.borrow().printed.clone()
    }
    fn said(&self, s: &str) -> bool {
        self.log.borrow().printed.iter().any(|p| p == s)
    }
    fn prompt(&self) -> String {
        self.log.borrow().last_prompt.clone()
    }
    fn sel(&self) -> Vec<u64> {
        self.log.borrow().selection.clone()
    }
    fn set_sel(&self, ids: &[u64]) {
        self.log.borrow_mut().selection = ids.to_vec();
    }
    fn undo_depth(&self) -> usize {
        self.eng.doc().undo_stack.len()
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6
}
fn ln(e: &Entity) -> Option<(f64, f64, f64, f64)> {
    if let Geo::Line(l) = &e.geo { Some((l.x1, l.y1, l.x2, l.y2)) } else { None }
}
fn lines(h: &H) -> Vec<(f64, f64, f64, f64)> {
    h.all().iter().filter_map(ln).collect()
}
fn circles(h: &H) -> Vec<(f64, f64, f64)> {
    h.all().iter().filter_map(|e| if let Geo::Circle(c) = &e.geo { Some((c.cx, c.cy, c.r)) } else { None }).collect()
}

// ---------------------------------------------------------------------------------------------------- Command line

#[test]
fn line_with_relative_polar_and_close_is_one_undo_step() {
    let mut h = H::new();
    h.ty(&["L", "0,0", "@10,0", "@0,10", "@10<180", "C"]);
    let l = lines(&h);
    assert_eq!(l.len(), 4, "LINE with relative, polar and Close → 4 lines");
    assert!(near(l[2].2, 0.0) && near(l[2].3, 10.0) && near(l[3].2, 0.0) && near(l[3].3, 0.0), "polar @10<180 and Close land right");
    assert_eq!(h.undo_depth(), 1, "the whole LINE command is one undo step");
    h.eng.doc_mut().undo();
    assert_eq!(h.count(), 0, "undo removes all four");
    h.eng.doc_mut().redo();
    assert_eq!(h.count(), 4, "redo brings them back");
    assert!(h.said("Command: LINE"), "command echoed in the history");
}

#[test]
fn line_undo_option_enter_repeats_and_direct_distance() {
    let mut h = H::new();
    h.ty(&["L", "0,0", "5,0", "5,5", "U", ""]);
    assert_eq!(h.count(), 1, "LINE Undo option removes the last segment");
    h.ty(&[""]);
    assert_eq!(h.eng.running(), Some("LINE"), "Enter at an empty prompt repeats the last command");
    h.log.borrow_mut().cursor = Some(P2::new(100.0, 0.0));
    h.ty(&["0,0", "25", ""]);
    let last = *lines(&h).last().unwrap();
    assert!(near(last.2, 25.0) && near(last.3, 0.0), "direct distance entry follows the cursor direction: {last:?}");
}

#[test]
fn draw_commands() {
    let mut h = H::new();
    h.ty(&["C", "0,0", "5"]);
    h.ty(&["CIRCLE", "2P", "0,0", "10,0"]);
    h.ty(&["C", "3P", "0,5", "5,0", "0,-5"]);
    let cs = circles(&h);
    assert!(
        cs.len() == 3 && near(cs[0].2, 5.0) && near(cs[1].2, 5.0) && near(cs[1].0, 5.0) && near(cs[2].2, 5.0) && near(cs[2].0, 0.0),
        "CIRCLE radius, 2P, 3P"
    );
    h.ty(&["A", "5,0", "0,5", "-5,0"]);
    let a = h.all().pop().unwrap();
    let mid = prim_point(&prims(&a, None)[0], 0.5);
    assert!(matches!(&a.geo, Geo::Arc(x) if near(x.r, 5.0)) && near(mid.y, 5.0), "ARC 3 points");
    h.ty(&["REC", "0,0", "@20,10"]);
    let r = h.all().pop().unwrap();
    assert!(
        matches!(&r.geo, Geo::Poly(p) if p.is_closed() && p.pts.len() == 4 && near(p.pts[2].x, 20.0) && near(p.pts[2].y, 10.0)),
        "RECTANG"
    );
    h.ty(&["POL", "6", "0,0", "I", "10,0"]);
    let p = h.all().pop().unwrap();
    assert!(matches!(&p.geo, Geo::Poly(p) if p.pts.len() == 6 && near(p.pts[0].x, 10.0) && near(p.pts[0].y, 0.0)), "POLYGON inscribed");
    h.ty(&["PL", "0,0", "10,0", "A", "10,10", "L", "0,10", "C"]);
    let pl = h.all().pop().unwrap();
    assert!(
        matches!(&pl.geo, Geo::Poly(p) if p.is_closed() && p.pts.len() == 4 && (p.pts[1].bulge() - 1.0).abs() < 1e-9),
        "PLINE with a tangent arc segment (half circle, bulge 1): {:?}",
        pl.to_value()
    );
    h.ty(&["EL", "-10,0", "10,0", "5"]);
    let el = h.all().pop().unwrap();
    assert!(matches!(&el.geo, Geo::Ellipse(e) if near(e.ratio, 0.5)), "ELLIPSE");
    h.ty(&["DT", "0,0", "2.5", "0", "Hello, world", "Second line", ""]);
    let all = h.all();
    let tx: Vec<_> = all.iter().rev().take(2).collect();
    match (&tx[1].geo, &tx[0].geo) {
        (Geo::Text(a), Geo::Text(b)) => {
            assert!(a.s == "Hello, world" && b.s == "Second line" && b.y < 0.0, "TEXT keeps spaces and commas; Enter starts the next line")
        }
        _ => panic!("two texts expected"),
    }
    assert!(!h.eng.busy(), "empty line ends TEXT");
}

#[test]
fn trim_move_copy_and_erase_by_window() {
    let mut h = H::new();
    h.ty(&["L", "0,0", "30,0", ""]);
    h.ty(&["L", "10,-5", "10,5", ""]);
    h.ty(&["L", "20,-5", "20,5", ""]);
    h.ty(&["TR"]);
    h.click(15.0, 0.0);
    h.ty(&[""]);
    assert!(h.count() == 4 && lines(&h).iter().filter(|l| l.1 == 0.0 && l.3 == 0.0).count() == 2, "TRIM removes the middle of the line");
    assert_eq!(h.eng.doc().undo_stack.last().map(|s| s.label.clone()), Some("TRIM".into()), "TRIM is one undo step");
    h.eng.doc_mut().undo();
    assert_eq!(h.count(), 3, "undo TRIM restores the line");

    let long = h.all().into_iter().find(|e| ln(e).is_some_and(|l| l.0 == 0.0 && l.2 == 30.0)).and_then(|e| e.id).unwrap();
    h.set_sel(&[long]);
    h.ty(&["M", "0,0", "0,100"]);
    let l = ln(h.eng.doc().get(long).unwrap()).unwrap();
    assert!(near(l.1, 100.0) && near(l.2, 30.0), "MOVE on a preselected object (noun-verb)");
    h.set_sel(&[]);
    h.ty(&["CO"]);
    h.click(10.0, 0.0);
    h.ty(&["", "10,0", "@5,0", "@10,0", ""]);
    assert_eq!(h.count(), 5, "COPY multiple");
    h.ty(&["E"]);
    assert!(h.sel().is_empty(), "a finished command clears the selection");
    h.eng.window(Bounds { min_x: 14.0, min_y: -6.0, max_x: 26.0, max_y: 6.0 }, false);
    h.ty(&[""]);
    assert!(h.count() == 2 && lines(&h).iter().all(|l| l.0 != 15.0 && l.0 != 20.0 && l.0 != 25.0), "ERASE by window");
}

#[test]
fn rotate_scale_mirror_offset_and_invalid_input() {
    let mut h = H::new();
    h.ty(&["L", "0,0", "10,0", ""]);
    let id = h.all()[0].id.unwrap();
    h.set_sel(&[id]);
    h.ty(&["RO", "0,0", "90"]);
    let l = lines(&h)[0];
    assert!(near(l.2, 0.0) && near(l.3, 10.0), "ROTATE 90");
    h.set_sel(&[id]);
    h.ty(&["SC", "0,0", "2"]);
    assert!(near(lines(&h)[0].3, 20.0), "SCALE 2");
    h.set_sel(&[id]);
    h.ty(&["MI", "5,0", "5,1", ""]);
    assert!(h.count() == 2 && near(lines(&h)[1].0, 10.0), "MIRROR keeps the source by default");
    h.ty(&["O", "3"]);
    h.click(0.0, 10.0);
    h.ty(&["2,10", ""]);
    assert!(h.count() == 3 && near(lines(&h)[2].0, 3.0), "OFFSET 3 to the picked side");
    h.ty(&["L", "0,0", "1,1"]);
    let before = h.count();
    h.ty(&["xyz"]);
    assert_eq!(h.count(), before, "invalid point input is refused, the command keeps waiting");
    assert_eq!(h.eng.expects(), Some(Kind::Point));
}

#[test]
fn unknown_command_and_esc() {
    let mut h = H::new();
    h.ty(&["NOSUCH"]);
    assert!(h.printed().iter().any(|l| l.contains("Unknown command")), "unknown command reported");
    h.ty(&["C", "0,0"]);
    h.eng.escape();
    assert!(!h.eng.busy() && h.said("*Cancel*"), "Esc cancels");
}

// ---------------------------------------------------------------------------------------------------- TRIM and EXTEND

fn comb(h: &mut H) {
    h.ty(&["L", "-5,0", "100,0", ""]);
    for i in 0..20 {
        let x = i * 5;
        h.ty(&["L", &format!("{x},-10"), &format!("{x},10"), ""]);
    }
}
fn uprights(h: &H) -> Vec<(f64, f64, f64, f64)> {
    lines(h).into_iter().filter(|l| l.0 == l.2).collect()
}
fn ghost_line(e: &Entity) -> Option<(f64, f64, f64, f64)> {
    (e.extra.get("ghost") == Some(&json!(true))).then(|| ln(e)).flatten()
}

#[test]
fn trim_by_drag_fence_preview_and_two_clicks() {
    let mut h = H::new();
    comb(&mut h);
    h.ty(&["TR"]);
    assert!(
        h.prompt().starts_with("Select object to trim or shift-select to extend") && h.prompt().contains("cuTting edges"),
        "TRIM asks for a piece, with its options: {}",
        h.prompt()
    );
    let steps0 = h.undo_depth();
    h.eng.path(&[P2::new(-2.0, 5.0), P2::new(40.0, 6.0), P2::new(98.0, 5.0)]);
    let ups = uprights(&h);
    assert!(
        ups.len() == 20 && ups.iter().all(|e| near(e.1.max(e.3), 0.0)) && h.count() == 21,
        "a dragged path trims all 20 top pieces back to the line"
    );
    assert!(h.undo_depth() == steps0 + 1 && h.said("Trimmed 20 objects."), "the whole path is one undo step, and says how many");
    h.ty(&["U"]);
    assert!(uprights(&h).iter().all(|e| near(e.1.max(e.3), 10.0)), "Undo inside TRIM puts all 20 back at once");
    h.ty(&["F", "-2,-5", "98,-5", ""]);
    assert!(
        uprights(&h).iter().all(|e| near(e.1.min(e.3), 0.0) && near(e.1.max(e.3), 10.0)),
        "F (fence) trims every bottom piece the fence crosses"
    );
    h.ty(&["U"]);
    let g1 = h.eng.preview(P2::new(2.5, 0.0));
    let g = g1.first().and_then(ghost_line);
    assert!(
        g1.len() == 1 && g.is_some_and(|g| near(g.0.min(g.2), 0.0) && near(g.0.max(g.2), 5.0)),
        "hover preview: the piece between two uprights, marked ghost"
    );
    h.eng.set_drag_path(Some(vec![P2::new(-2.0, 5.0), P2::new(22.0, 5.0)]));
    let g2 = h.eng.preview(P2::new(22.0, 5.0));
    h.eng.set_drag_path(None);
    assert!(
        g2.len() == 5 && g2.iter().all(|e| ghost_line(e).is_some_and(|l| near(l.1.min(l.3), 0.0) && near(l.1.max(l.3), 10.0))),
        "drag preview: the five top pieces the path has crossed so far ({})",
        g2.len()
    );
    assert!(h.eng.preview(P2::new(300.0, 300.0)).is_empty(), "no preview over empty space");
    h.click(-3.0, -6.0);
    h.click(12.0, -6.0);
    assert_eq!(
        uprights(&h).iter().filter(|e| near(e.1.min(e.3), 0.0)).count(),
        3,
        "two clicks on empty spots trim along the line between them (3 bottoms)"
    );
    h.ty(&[""]);
    assert!(!h.eng.busy(), "Enter ends TRIM");
}

#[test]
fn shift_drag_in_trim_extends_and_shift_click_in_extend_trims() {
    let mut h = H::new();
    h.ty(&["L", "10,-1", "10,10", ""]);
    for k in 1..=5 {
        h.ty(&["L", &format!("0,{k}"), &format!("5,{k}"), ""]);
    }
    h.ty(&["TR"]);
    h.log.borrow_mut().shift = true;
    let gx = h.eng.preview(P2::new(4.9, 3.0));
    let g = gx.first().and_then(ghost_line);
    assert!(
        gx.len() == 1 && g.is_some_and(|g| near(g.0.min(g.2), 5.0) && near(g.0.max(g.2), 10.0)),
        "Shift held in TRIM: the preview is the part an extend would add (5 → 10)"
    );
    h.eng.path(&[P2::new(3.0, 0.0), P2::new(3.0, 6.0)]);
    h.log.borrow_mut().shift = false;
    let runs: Vec<_> = lines(&h).into_iter().filter(|e| e.1 == e.3).collect();
    assert!(runs.len() == 5 && runs.iter().all(|e| near(e.0.max(e.2), 10.0)), "Shift+drag in TRIM extends the five lines to the boundary");
    h.ty(&[""]);
    h.ty(&["L", "-10,7", "30,7", ""]);
    h.ty(&["EX"]);
    assert!(h.prompt().starts_with("Select object to extend or shift-select to trim"), "EXTEND asks with its own wording");
    h.log.borrow_mut().shift = true;
    h.click(20.0, 7.0);
    h.log.borrow_mut().shift = false;
    let top = lines(&h).into_iter().find(|e| e.1 == 7.0 && e.3 == 7.0).unwrap();
    assert!(near(top.0.max(top.2), 10.0), "Shift+click in EXTEND trims instead");
    h.ty(&[""]);
}

#[test]
fn quick_mode_erases_what_nothing_crosses_standard_keeps_it() {
    let mut h = H::new();
    h.ty(&["L", "0,0", "30,0", "", "L", "10,-5", "10,5", "", "L", "20,-5", "20,5", "", "L", "0,50", "10,50", ""]);
    h.ty(&["TR"]);
    assert!(h.said("Current settings: Quick mode, edges: No extend"), "TRIM says its settings (Quick, no extend)");
    h.click(5.0, 50.0);
    assert!(
        h.count() == 3 && h.said("Nothing crossed 1 object, so it was erased (Quick mode; mOde, Standard keeps such objects)."),
        "Quick mode: a line nothing crosses is erased, and says so"
    );
    h.ty(&["U", "O", "S"]);
    h.click(10.0, 3.0);
    h.ty(&[""]);
    assert!(h.said("1 edge chosen."), "mOde Standard asks for cutting edges (one chosen)");
    h.click(25.0, 0.0);
    let base: Vec<_> = lines(&h).into_iter().filter(|e| e.1 == 0.0 && e.3 == 0.0).collect();
    assert!(base.len() == 1 && near(base[0].0.max(base[0].2), 10.0), "Standard: only the chosen edge cuts, so 10 → 30 goes in one piece");
    h.click(5.0, 50.0);
    assert!(
        h.count() == 4 && h.said("Nothing to trim there: no chosen edge crosses that object."),
        "Standard: an object no edge crosses is left alone"
    );
    h.ty(&["O", "Q", ""]);
    assert!(!h.eng.state().trim_standard, "mOde Quick goes back (kept for the session)");
}

#[test]
fn crossing_edge_extend_and_erase() {
    let mut h = H::new();
    h.ty(&["L", "0,0", "30,0", "", "L", "10,-5", "10,5", "", "L", "20,-5", "20,5", ""]);
    h.ty(&["TR", "C", "12,-1", "18,1"]);
    assert_eq!(lines(&h).iter().filter(|e| e.1 == 0.0 && e.3 == 0.0).count(), 2, "C (crossing) removes the piece inside the window");
    h.ty(&[""]);
    let mut g = H::new();
    g.ty(&["L", "0,0", "30,0", "", "L", "10,1", "10,5", ""]);
    g.ty(&["TR", "E", "E"]);
    assert!(g.said("Current settings: Quick mode, edges: Extend"), "Edge Extend is said");
    g.click(5.0, 0.0);
    let left = lines(&g).into_iter().find(|e| e.1 == 0.0 && e.3 == 0.0).unwrap();
    assert!(near(left.0.min(left.2), 10.0) && near(left.0.max(left.2), 30.0), "with Edge Extend a short line cuts where it would reach");
    g.ty(&["R"]);
    g.click(10.0, 3.0);
    g.ty(&[""]);
    assert_eq!(g.count(), 1, "eRase erases without leaving TRIM");
    assert_eq!(g.eng.running(), Some("TRIM"), "and TRIM goes on");
    g.ty(&[""]);
    g.ty(&["TR", "E", "N", ""]);
    assert_eq!(g.eng.state().edge_mode, 0);
}

#[test]
fn trim_construction_lines() {
    let mut h = H::load(json!({ "entities": [{ "type": "xline", "layer": "0", "x": 0, "y": 50, "dx": 1, "dy": 0 }] }));
    h.ty(&["L", "120,-100", "120,300", "", "L", "0,0", "200,100", ""]);
    h.ty(&["TR"]);
    h.click(170.0, 50.0);
    h.ty(&[""]);
    let all = h.all();
    let r = all.iter().find_map(|e| if let Geo::Ray(r) = &e.geo { Some(r.clone()) } else { None }).unwrap();
    assert!(
        near(r.x, 120.0) && near(r.y, 50.0) && r.dx < 0.0 && !all.iter().any(|e| matches!(e.geo, Geo::Xline(_))),
        "TRIM by a click: the right part of an xline goes, a ray from the cut is left"
    );
    h.ty(&["TR"]);
    h.click(110.0, 55.0);
    h.ty(&[""]);
    let cut = lines(&h).into_iter().find(|e| near(e.0, 0.0)).unwrap();
    assert!(near(cut.2, 100.0) && near(cut.3, 50.0), "an ordinary line is cut by the ray (a construction line is a cutting edge too)");
}

// ---------------------------------------------------------------------------------------------------- selection options

fn five(h: &mut H) {
    for i in 0..5 {
        let x = i * 10;
        h.ty(&["L", &format!("{x},0"), &format!("{x},10"), ""]);
    }
}

#[test]
fn selection_fence_and_help() {
    let mut h = H::new();
    five(&mut h);
    h.ty(&["E"]);
    assert_eq!(h.prompt(), "Select objects:", "the selection options stay out of the prompt");
    h.ty(&["?"]);
    assert!(h.said("Click objects or drag a window, then press Enter."), "? lists what \"Select objects\" takes");
    h.ty(&["F", "-5,5", "15,5", "", ""]);
    assert_eq!(h.count(), 3, "Fence: the two lines the fence crosses are erased");
    assert!(h.said("2 found"), "Fence reports how many it found");
}

#[test]
fn selection_remove_add_crossing() {
    let mut h = H::new();
    five(&mut h);
    h.ty(&["E", "ALL", "R"]);
    assert_eq!(h.prompt(), "Remove objects:", "R switches to \"Remove objects\"");
    h.click(20.0, 5.0);
    assert_eq!(h.sel().len(), 4, "a click in Remove mode takes the object out");
    h.ty(&["A"]);
    h.click(20.0, 5.0);
    assert_eq!(h.sel().len(), 5, "A adds again");
    h.ty(&["R", "C", "25,-1", "45,11"]);
    assert!(h.sel().len() == 3 && h.said("2 removed"), "Crossing in Remove mode takes out what it touches");
    h.ty(&[""]);
    assert_eq!(h.count(), 2, "and ERASE erases the rest");
}

#[test]
fn selection_window_and_previous() {
    let mut h = H::new();
    five(&mut h);
    h.ty(&["M", "W", "-1,-1", "11,11", "", "0,0", "100,0"]);
    assert_eq!(lines(&h).iter().filter(|e| e.0 >= 100.0).count(), 2, "Window: what is fully inside is moved");
    h.ty(&["M", "P", "", "0,0", "0,50"]);
    assert_eq!(lines(&h).iter().filter(|e| e.1 >= 50.0).count(), 2, "Previous: the same two objects again");
    h.ty(&["E", "P", ""]);
    assert_eq!(h.count(), 3, "Previous after a move erases them");
    h.ty(&["E", "P"]);
    assert!(h.said("There is no previous selection yet."), "Previous with nothing left says so");
    h.ty(&[""]);
}

#[test]
fn fence_undo_and_esc() {
    let mut h = H::new();
    five(&mut h);
    h.ty(&["E", "F", "-5,5", "5,5", "U", "-5,8", "15,8", "U"]);
    assert!(h.eng.running() == Some("ERASE") && h.eng.expects() == Some(Kind::Point), "Undo takes back the last fence point");
    h.eng.escape();
    assert!(h.count() == 5 && !h.eng.busy(), "Esc while fencing cancels the command");
}

// ---------------------------------------------------------------------------------------------------- ZOOM

#[test]
fn zoom_previous_scale_center() {
    let mut h = H::new();
    h.log.borrow_mut().view = Some((0.0, 0.0, 100.0, 50.0));
    h.ty(&["Z", "2x"]);
    h.ty(&["Z", "0.5"]);
    h.ty(&["Z", "S", "3X"]);
    assert_eq!(h.log.borrow().calls.join("|"), "by 2|by 0.5|by 3", "ZOOM 2x, 0.5 and Scale 3X zoom by a factor");
    h.ty(&["Z", "P"]);
    h.ty(&["Z", "P"]);
    assert!(h.said("There is no previous view yet."), "ZOOM P goes back; with nothing left it says so");
    h.ty(&["Z", "C", "10,10", "20"]);
    assert!(
        h.log.borrow().calls.contains(&"win -10 0 30 20 true".to_string()),
        "ZOOM Center: point and height (the width follows the screen)"
    );
    h.ty(&["Z", "E"]);
    h.ty(&["Z", ""]);
    assert_eq!(h.log.borrow().calls.iter().filter(|c| *c == "extents").count(), 2, "ZOOM Extents, and Enter");
    h.ty(&["Z", "W", "0,0", "4,3"]);
    assert!(h.log.borrow().calls.contains(&"win 0 0 4 3 false".to_string()), "ZOOM Window");
}

// ---------------------------------------------------------------------------------------------------- tangents

fn tan(x: f64, y: f64, cx: f64, cy: f64, r: f64, nx: f64, ny: f64) -> (P2, Tan) {
    (P2::new(x, y), Tan { c: Circ { cx, cy, r }, near: P2::new(nx, ny) })
}

#[test]
fn line_from_deferred_tangents() {
    let mut h = H::new();
    h.ty(&["L"]);
    let (p, t) = tan(0.0, 5.0, 0.0, 0.0, 5.0, 0.0, 6.0);
    h.eng.click(p, Some(t), p);
    let (p, t) = tan(20.0, 5.0, 20.0, 0.0, 5.0, 20.0, 6.0);
    h.eng.click(p, Some(t), p);
    h.ty(&[""]);
    let l = lines(&h);
    assert!(
        l.len() == 1 && near(l[0].1, 5.0) && near(l[0].3, 5.0) && near(l[0].0, 0.0) && near(l[0].2, 20.0),
        "LINE from Tangent to Tangent: the line touching both circles"
    );
    h.ty(&["L"]);
    let (p, t) = tan(0.0, 5.0, 0.0, 0.0, 5.0, 0.0, 6.0);
    h.eng.click(p, Some(t), p);
    h.ty(&["10,0", ""]);
    let l2 = lines(&h)[1];
    assert!(
        near(l2.0.hypot(l2.1), 5.0) && near(l2.0, 2.5) && l2.1 > 0.0 && near(l2.2, 10.0),
        "LINE from Tangent to a point: the touch point on the picked side"
    );
}

fn tl(x1: f64, y1: f64, x2: f64, y2: f64) -> Curve {
    let l = (x2 - x1).hypot(y2 - y1);
    Curve::L { x: x1, y: y1, dx: (x2 - x1) / l, dy: (y2 - y1) / l }
}
fn tc(cx: f64, cy: f64, r: f64) -> Curve {
    Curve::C { cx, cy, r }
}
fn p(x: f64, y: f64) -> P2 {
    P2::new(x, y)
}
fn eq(k: Option<cc_engine::tancircle::Circle>, x: f64, y: f64, r: f64) -> bool {
    k.is_some_and(|k| near(k.cx, x) && near(k.cy, y) && near(k.r, r))
}

#[test]
fn tangent_circle_math() {
    assert!(
        eq(circle_ttr(&tl(0.0, 0.0, 10.0, 0.0), p(5.0, 0.0), &tl(0.0, 0.0, 0.0, 10.0), p(0.0, 5.0), 2.0), 2.0, 2.0, 2.0),
        "TTR line–line: r 2 in the corner picked"
    );
    assert!(
        eq(circle_ttr(&tl(0.0, 0.0, 10.0, 0.0), p(5.0, 0.0), &tl(0.0, 0.0, 0.0, -10.0), p(0.0, -5.0), 2.0), 2.0, -2.0, 2.0),
        "… the other corner"
    );
    assert!(
        eq(circle_ttr(&tl(-20.0, 0.0, 20.0, 0.0), p(3.0, 0.0), &tc(0.0, 10.0, 5.0), p(2.0, 5.5), 3.0), 15f64.sqrt(), 3.0, 3.0),
        "TTR line–circle"
    );
    assert!(
        eq(circle_ttr(&tc(0.0, 0.0, 5.0), p(4.0, 3.0), &tc(20.0, 0.0, 5.0), p(16.0, 3.0), 6.0), 10.0, 21f64.sqrt(), 6.0),
        "TTR circle–circle outside"
    );
    let ins = circle_ttr(&tc(0.0, 0.0, 20.0), p(0.0, 19.0), &tc(0.0, 12.0, 3.0), p(0.0, 15.2), 4.0).unwrap();
    assert!(near(ins.cx.hypot(ins.cy), 16.0) && near(ins.cx.hypot(ins.cy - 12.0), 7.0), "TTR circle–circle inside");
    assert!(circle_ttr(&tc(0.0, 0.0, 5.0), p(4.0, 3.0), &tc(40.0, 0.0, 5.0), p(36.0, 3.0), 6.0).is_none(), "TTR too far apart: none");
    let inr = 10.0 - SQRT_2 * 5.0;
    assert!(
        eq(
            circle_ttt(
                &tl(0.0, 0.0, 10.0, 0.0),
                p(3.0, 0.0),
                &tl(0.0, 0.0, 0.0, 10.0),
                p(0.0, 3.0),
                &tl(10.0, 0.0, 0.0, 10.0),
                p(5.0, 5.0)
            ),
            inr,
            inr,
            inr
        ),
        "TTT incircle"
    );
    let ex =
        circle_ttt(&tl(0.0, 0.0, 10.0, 0.0), p(14.0, 0.0), &tl(0.0, 0.0, 0.0, 10.0), p(0.0, 14.0), &tl(10.0, 0.0, 0.0, 10.0), p(7.0, 3.0))
            .unwrap();
    assert!(ex.r > 10.0 && ex.cx > 10.0 && ex.cy > 10.0, "TTT an excircle");
    let cs = [tc(0.0, 0.0, 1.0), tc(10.0, 0.0, 1.0), tc(5.0, 8.0, 1.0)];
    let k = circle_ttt(&cs[0], p(0.6, 0.8), &cs[1], p(9.4, 0.8), &cs[2], p(5.0, 7.0)).unwrap();
    assert!(
        cs.iter().all(|o| {
            let Curve::C { cx, cy, .. } = o else { return false };
            let d = (k.cx - cx).hypot(k.cy - cy);
            near(d, k.r + 1.0) || near(d, (k.r - 1.0).abs())
        }),
        "TTT three circles"
    );
    let mix =
        circle_ttt(&tl(-20.0, 0.0, 20.0, 0.0), p(0.0, 0.0), &tc(-6.0, 6.0, 2.0), p(-4.0, 6.0), &tc(6.0, 6.0, 2.0), p(4.0, 6.0)).unwrap();
    assert!(near(mix.cy.abs(), mix.r) && near((mix.cx + 6.0).hypot(mix.cy - 6.0), mix.r + 2.0), "TTT a line and two circles");
    assert!(
        circle_ttt(&tl(0.0, 0.0, 10.0, 0.0), p(3.0, 0.0), &tl(0.0, 5.0, 10.0, 5.0), p(3.0, 5.0), &tl(0.0, 10.0, 10.0, 10.0), p(5.0, 10.0))
            .is_none(),
        "TTT parallel lines: none"
    );
}

#[test]
fn tangent_circle_commands() {
    let mut h = H::new();
    h.ty(&["L", "0,0", "100,0", "", "L", "0,0", "0,100", ""]);
    h.ty(&["C", "T"]);
    h.click(50.0, 0.0);
    h.click(0.0, 50.0);
    h.ty(&["10"]);
    let cs = circles(&h);
    assert!(cs.len() == 1 && near(cs[0].0, 10.0) && near(cs[0].1, 10.0) && near(cs[0].2, 10.0), "CIRCLE T: two clicks and a radius");
    h.ty(&["TTR"]);
    h.click(50.0, 0.0);
    h.click(0.0, 50.0);
    h.ty(&[""]);
    let cs = circles(&h);
    assert!(cs.len() == 2 && near(cs[1].2, 10.0), "TTR takes the last radius on Enter");
    h.ty(&["L", "100,0", "0,100", ""]);
    h.ty(&["C", "TTT"]);
    h.click(30.0, 0.0);
    h.click(0.0, 30.0);
    h.click(50.0, 50.0);
    let cs = circles(&h);
    let r = 100.0 - 50.0 * SQRT_2;
    assert!(
        cs.len() == 3 && near(cs[2].0, r) && near(cs[2].1, r) && near(cs[2].2, r),
        "CIRCLE TTT: the triangle's incircle by three picks"
    );
    h.ty(&["C", "300,0", "5", "C", "340,0", "5"]);
    h.ty(&["C", "T"]);
    h.click(304.0, 3.0);
    h.click(336.0, 3.0);
    h.ty(&["6"]);
    assert!(h.said("Circle does not exist."), "no circle of that radius fits");
}

// ---------------------------------------------------------------------------------------------------- UCS

#[test]
fn typed_points_and_text_follow_the_ucs() {
    let mut h = H::load(json!({ "ucs": { "current": { "x": 0, "y": 0, "ang": PI / 2.0 } } }));
    h.ty(&["L", "0,0", "10,0", "@0,5", ""]);
    let ls = lines(&h);
    assert!(
        ls.len() == 2 && near(ls[0].2, 0.0) && near(ls[0].3, 10.0) && near(ls[1].2, -5.0) && near(ls[1].3, 10.0),
        "typed x,y and @dx,dy are in the UCS"
    );
    h.ty(&["L", "10<0", "*10,0", ""]);
    let l3 = lines(&h)[2];
    assert!(
        near(l3.0, 0.0) && near(l3.1, 10.0) && near(l3.2, 10.0) && near(l3.3, 0.0),
        "d<a from the UCS origin along its X axis; * means world coordinates"
    );
    let mut t = H::load(json!({ "ucs": { "current": { "x": 0, "y": 0, "ang": PI / 6.0 } } }));
    t.ty(&["DT", "0,0", "2.5", "", "Along the UCS", ""]);
    let tx = t.all().pop().unwrap();
    assert!(
        matches!(&tx.geo, Geo::Text(x) if near(x.rot.unwrap_or(0.0), PI / 6.0)),
        "new text runs along the UCS X axis (its default angle)"
    );
}

// ---------------------------------------------------------------------------------------------------- engine edges

#[test]
fn polygon_sides_are_capped_and_hostile_input_is_refused() {
    let mut h = H::new();
    h.ty(&["POL", "1e9"]);
    assert!(!h.eng.busy() && h.count() == 0, "a huge side count is refused, not attempted");
    h.ty(&["POL", "1024", "0,0", "", "5,0"]);
    assert!(matches!(&h.all()[0].geo, Geo::Poly(p) if p.pts.len() == 1024));
    for junk in ["@", "<", "1,,2", "\u{0}", "9e999,1", "@1e308<1e308", "--5", "1/0,1", "'", "\"", "#*@", "1'2'3\""] {
        h.ty(&["L", junk]);
        h.eng.escape();
    }
    assert_eq!(h.count(), 1);
}

#[test]
fn escape_mid_select_option_restores_and_ends() {
    let mut h = H::new();
    five(&mut h);
    h.ty(&["M", "W", "0,0"]);
    assert!(h.eng.escape());
    assert!(!h.eng.busy() && h.said("*Cancel*"));
    assert!(!h.eng.escape(), "Esc with nothing waiting does nothing");
    h.ty(&["U"]);
    assert_eq!(h.count(), 4, "UNDO takes back the last LINE");
    h.ty(&["REDO"]);
    assert_eq!(h.count(), 5);
    assert!(h.said("Redid LINE"));
}
