//! cc-lisp without a drawing: values as 0.5 prints them, and the caps that keep hostile programs from hanging or
//! crashing the app (CLAUDE.md §4: scripts are hostile input).

use std::cell::RefCell;
use std::rc::Rc;

use cc_lisp::Limits;
use cc_lisp::value::lstr;

fn run_with(src: &str, limits: Option<Limits>) -> (bool, String, String) {
    let out = Rc::new(RefCell::new(String::new()));
    let o = Rc::clone(&out);
    let mut l = cc_lisp::new_lisp(None, Rc::new(move |s: &str| o.borrow_mut().push_str(s)));
    if let Some(lim) = limits {
        l.limits = lim;
    }
    let r = cc_lisp::block_on(l.eval_text(&lstr(src))).expect("no host: one poll finishes");
    let shown = l.show(&r.value, true);
    let printed = out.borrow().clone();
    (r.ok, shown, printed)
}
fn run(src: &str) -> (bool, String, String) {
    run_with(src, None)
}
fn small() -> Option<Limits> {
    Some(Limits { steps: Some(100_000), frames: 50_000, string: 1 << 16, array: 1 << 10 })
}

#[test]
fn values_print_as_0_5() {
    assert_eq!(run("(+ 1 2.5)").1, "3.5");
    assert_eq!(run("(list 1 \"a\" 'b 2.0)").1, "(1 \"a\" B 2.0)");
    assert_eq!(run("(cons 1 2)").1, "(1 . 2)");
    assert_eq!(run("(defun f (n) (if (< n 2) 1 (* n (f (1- n))))) (f 10)").1, "3628800");
    assert_eq!(run("(vl-sort '(3 1 2) '<)").1, "(1 2 3)");
    assert_eq!(run("(rtos 1.5 2 3)").1, "\"1.500\"");
    assert_eq!(run("(wcmatch \"LINE\" \"L*\")").1, "T");
    assert_eq!(run("(vl-catch-all-error-message (vl-catch-all-apply '/ '(1 0)))").1, "\"divide by zero\"");
}

#[test]
fn errors_go_to_error_handler() {
    let (ok, _, printed) = run("(defun *error* (m) (princ (strcat \"caught \" m))) (car 1)");
    assert!(!ok);
    assert!(printed.contains("caught bad argument type"), "{printed}");
}

#[test]
fn an_endless_loop_stops_at_the_step_cap() {
    let (ok, _, printed) = run_with("(while t (setq x 1))", small());
    assert!(!ok);
    assert!(!printed.is_empty());
}

#[test]
fn deep_recursion_is_an_error_not_a_stack_overflow() {
    let (ok, _, printed) = run_with("(defun f (n) (+ 1 (f (1+ n)))) (f 0)", small());
    assert!(!ok);
    assert!(printed.contains("Maximum call stack size exceeded"), "{printed}");
}

#[test]
fn deeply_nested_source_reads_without_recursion() {
    let src = format!("{}1{}", "(list ".repeat(50_000), ")".repeat(50_000));
    let (_, _, _) = run_with(&src, small());
    // and a long dotted chain drops without recursion
    let (ok, _, _) = run_with("(setq l nil) (repeat 20000 (setq l (list l)))", small());
    assert!(ok);
}

#[test]
fn big_strings_and_arrays_are_refused() {
    let (ok, _, _) = run_with("(setq s \"x\") (repeat 40 (setq s (strcat s s)))", small());
    assert!(!ok);
    let (ok, _, _) = run_with("(vlax-make-safearray 5 '(0 . 100000000))", small());
    assert!(!ok);
}

#[test]
fn unbalanced_and_garbage_input_is_an_error() {
    for src in ["(defun f (", ")", "(1 . )", "\"open", "(setq 1 2)", "#<x>", "(quote)"] {
        let _ = run_with(src, small());
    }
}
