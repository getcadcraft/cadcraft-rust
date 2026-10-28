//! LISP on a drawing through the engine (appload.js / install.js / host-cad.js): routines loaded as applications,
//! (command …) driving commands, prompts answered at the command line, one undo step per routine, and the caps.

use std::cell::RefCell;
use std::rc::Rc;

use cc_cad::Doc;
use cc_cad::entity::Entity;
use cc_cad_geom::geom::Bounds;
use cc_engine::lisp::AppFile;
use cc_engine::{Engine, Host, Lang};
use cc_geom::P2;

#[derive(Default)]
struct Log {
    printed: Vec<String>,
    selection: Vec<u64>,
}
struct Stub(Rc<RefCell<Log>>);
impl Host for Stub {
    fn print(&mut self, s: &str) {
        self.0.borrow_mut().printed.push(s.into());
    }
    fn prompt(&mut self, _s: &str) {}
    fn cursor(&self) -> Option<P2> {
        None
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
}

fn engine() -> (Engine, Rc<RefCell<Log>>) {
    let log = Rc::new(RefCell::new(Log::default()));
    (Engine::new(Doc::new(), Box::new(Stub(Rc::clone(&log))), Lang::default()), log)
}
fn load(eng: &mut Engine, text: &str) {
    eng.lisp_load(vec![AppFile { name: "t.lsp".into(), text: text.into(), kind: "lsp".into() }], "APPLOAD");
}
fn count(eng: &Engine) -> usize {
    eng.doc().entities().count()
}

#[test]
fn a_routine_draws_through_commands_as_one_undo_step() {
    let (mut eng, log) = engine();
    load(
        &mut eng,
        "(defun c:SQ (/ p) (setq p '(0 0)) (command \"LINE\" p '(4 0) '(4 4) '(0 4) \"C\") (entmake '((0 . \"CIRCLE\") (10 2 2) (40 . 1))) (princ))",
    );
    assert!(log.borrow().printed.iter().any(|l| l.contains("appload.loaded")), "{:?}", log.borrow().printed);
    eng.text("SQ");
    assert!(!eng.busy());
    assert_eq!(count(&eng), 5);
    eng.run("U");
    assert_eq!(count(&eng), 0);
}

#[test]
fn prompts_are_answered_at_the_command_line() {
    let (mut eng, _) = engine();
    load(
        &mut eng,
        "(defun c:C2 (/ p r) (setq p (getpoint \"Center: \")) (initget 7) (setq r (getdist p \"Radius: \")) (command \"CIRCLE\" p r) (princ))",
    );
    eng.text("C2");
    assert!(eng.busy());
    eng.text("5,5");
    eng.text("-1"); // refused by (initget 7): asked again
    assert!(eng.busy());
    eng.text("2");
    assert!(!eng.busy());
    assert_eq!(count(&eng), 1);
}

#[test]
fn typed_lisp_lines_and_a_value_at_a_prompt() {
    let (mut eng, log) = engine();
    assert!(eng.lisp_line("(setq r 3)"));
    assert!(eng.lisp_typing("(defun"));
    assert!(eng.lisp_line("(defun c:HI ()"));
    assert!(eng.lisp_line("(princ \"hello\") (princ))"));
    eng.text("HI");
    assert!(log.borrow().printed.iter().any(|l| l.contains("hello")));
    eng.text("CIRCLE");
    eng.text("0,0");
    assert!(eng.lisp_line("(* r 2)"));
    assert!(!eng.busy());
    assert_eq!(count(&eng), 1);
    assert!(!eng.lisp_line("LINE"));
}

#[test]
fn an_endless_routine_stops_at_the_cap_and_the_drawing_is_kept_whole() {
    let (mut eng, log) = engine();
    eng.set_lisp_limits(cc_lisp::Limits { steps: Some(200_000), frames: 50_000, string: 1 << 16, array: 1 << 10 });
    load(&mut eng, "(defun c:LOOP () (entmake '((0 . \"POINT\") (10 1 1))) (while t (setq x 1)))");
    eng.text("LOOP");
    assert!(!eng.busy());
    assert!(log.borrow().printed.iter().any(|l| l.contains("error")), "{:?}", log.borrow().printed);
    // what it made before the error is one undo step
    eng.run("U");
    assert_eq!(count(&eng), 0);
}

#[test]
fn compiled_or_broken_files_never_crash() {
    let (mut eng, log) = engine();
    load(&mut eng, "(defun c:X () (");
    load(&mut eng, "\u{0}\u{1}((((((");
    load(&mut eng, &"(".repeat(200_000));
    assert!(!eng.busy());
    assert!(!log.borrow().printed.is_empty());
}
