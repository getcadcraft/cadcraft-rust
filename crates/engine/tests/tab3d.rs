//! The 3D tab (ws3d.rs, M4.25) driven headless: opening it, a push/pull by clicks and by typing, MOVE on the 3D
//! selection, a boolean, undo through the drawing's history, and drawing units that scale the solids.

use std::cell::RefCell;
use std::rc::Rc;

use cc_cad::Doc;
use cc_cad::entity::Entity;
use cc_cad_geom::geom::Bounds;
use cc_engine::ws3d::Ev3;
use cc_engine::{Engine, Host, Lang};
use cc_geom::P2;

#[derive(Default)]
struct Log {
    printed: Vec<String>,
    cursor: Option<P2>,
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
    fn selection(&self) -> Vec<u64> {
        self.0.borrow().selection.clone()
    }
    fn set_selection(&mut self, ids: &[u64]) {
        self.0.borrow_mut().selection = ids.to_vec();
    }
    fn pick_at(&self, _doc: &Doc, _p: P2) -> Option<u64> {
        None
    }
    fn pick_window(&self, _doc: &Doc, _w: &Bounds, _crossing: bool) -> Vec<u64> {
        Vec::new()
    }
    fn preview(&mut self, _ents: Vec<Entity>) {}
    fn zoom_extents(&mut self) {}
    fn zoom_window(&mut self, _a: P2, _b: P2, _exact: bool) {}
    fn open3d(&mut self) -> bool {
        true
    }
    fn set_cursor(&mut self, p: P2) {
        self.0.borrow_mut().cursor = Some(p);
    }
}

fn lang() -> Lang {
    let en: serde_json::Value = serde_json::from_str(include_str!("../../../assets/lang/cadcraft/en.json")).unwrap_or_default();
    Lang::new(&en)
}

fn engine() -> (Engine, Rc<RefCell<Log>>) {
    let log = Rc::new(RefCell::new(Log { cursor: Some(P2::new(0.0, 0.0)), ..Log::default() }));
    (Engine::new(Doc::new(), Box::new(Stub(Rc::clone(&log))), lang()), log)
}

fn ty(e: &mut Engine, lines: &[&str]) {
    for l in lines {
        e.text(l);
    }
}

/// Where the 3D camera shows a drawing point (drawing units, unitless = mm).
fn px(e: &Engine, p: [f64; 3]) -> (f64, f64) {
    e.with_ws3d(|ws, _| ws.tools.host.cam.project([p[0] * 0.001, p[1] * 0.001, p[2] * 0.001]).map(|q| (q[0], q[1])))
        .flatten()
        .unwrap_or((f64::NAN, f64::NAN))
}

fn click(e: &mut Engine, p: [f64; 3], t: f64) {
    let (x, y) = px(e, p);
    e.ws_click(Ev3 { x, y, time: Some(t), ..Ev3::default() });
}

fn m3d(e: &Engine) -> String {
    e.doc().m3d.as_deref().unwrap_or("").to_string()
}

#[test]
fn presspull_move_union_undo_on_the_3d_tab() {
    let (mut e, log) = engine();
    ty(&mut e, &["BOX", "0,0", "10,10", "5"]);
    assert!(!e.in3d());
    assert!(e.open3d(), "the stub has a 3D tab");
    assert!(e.in3d());
    let one = m3d(&e);
    // PRESSPULL: the top face clicked, 3 typed
    ty(&mut e, &["PRESSPULL"]);
    click(&mut e, [5.0, 5.0, 5.0], 1000.0);
    ty(&mut e, &["3"]);
    assert!(log.borrow().printed.iter().any(|p| p == "Pushed or pulled one face."), "{:?}", log.borrow().printed);
    assert!(!e.busy());
    let pulled = m3d(&e);
    assert_ne!(pulled, one);
    // one undo step, through the drawing's history
    ty(&mut e, &["U"]);
    assert_eq!(m3d(&e), one);
    ty(&mut e, &["REDO"]);
    assert_eq!(m3d(&e), pulled);
    // MOVE on the 3D selection, typed
    click(&mut e, [5.0, 5.0, 8.0], 3000.0);
    assert_eq!(e.with_ws3d(|ws, _| ws.tools.host.sel.len()), Some(1));
    ty(&mut e, &["MOVE", "@20,0"]);
    assert!(log.borrow().printed.iter().any(|p| p == "Moved."));
    // a second solid overlapping the first, then UNION of everything
    ty(&mut e, &["DESELECT", "BOX", "25,5", "35,15", "4", "UNION", "ALL", ""]);
    assert!(log.borrow().printed.iter().any(|p| p.starts_with("2 solids joined into one")), "{:?}", log.borrow().printed);
    // the units scale the model (one undo step with them)
    let before = m3d(&e);
    let (scaled, ratio) = cc_engine::cmd::solid3d::set_units(e.shared(), "meters");
    assert!(scaled && (ratio - 1000.0).abs() < 1e-9);
    ty(&mut e, &["U"]);
    assert_eq!(m3d(&e), before);
    // a sheet hides the tab
    e.hide3d();
    assert!(!e.in3d());
}

#[test]
fn needs_the_3d_tab_without_one() {
    struct No(Stub);
    impl Host for No {
        fn print(&mut self, s: &str) {
            self.0.print(s);
        }
        fn prompt(&mut self, s: &str) {
            self.0.prompt(s);
        }
        fn cursor(&self) -> Option<P2> {
            self.0.cursor()
        }
        fn selection(&self) -> Vec<u64> {
            self.0.selection()
        }
        fn set_selection(&mut self, ids: &[u64]) {
            self.0.set_selection(ids);
        }
        fn pick_at(&self, d: &Doc, p: P2) -> Option<u64> {
            self.0.pick_at(d, p)
        }
        fn pick_window(&self, d: &Doc, w: &Bounds, c: bool) -> Vec<u64> {
            self.0.pick_window(d, w, c)
        }
        fn preview(&mut self, e: Vec<Entity>) {
            self.0.preview(e);
        }
        fn zoom_extents(&mut self) {}
        fn zoom_window(&mut self, _a: P2, _b: P2, _x: bool) {}
    }
    let log = Rc::new(RefCell::new(Log::default()));
    let mut e = Engine::new(Doc::new(), Box::new(No(Stub(Rc::clone(&log)))), lang());
    ty(&mut e, &["VIEW"]);
    assert!(log.borrow().printed.iter().any(|p| p == "This needs the 3D tab, which is not available here."));
    assert!(!e.busy());
}
