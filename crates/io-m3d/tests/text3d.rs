//! 3D text (text3d.js, 0.4 M11): the frozen test-modelcraft.mjs section "3D text", ported. Exact parity with the 0.5
//! JS (the saved files) is checked by `forks/tools/mcuidiff.mjs --edit`.

use cc_io_m3d::text3d::text_def;
use cc_m3d::Model;
use cc_m3d::def::face_normal;

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn flat_letters_keep_their_holes() {
    let mut model = Model::new();
    model.units = "m".into();
    let id = model.change("t", |m| text_def(m, "Hello O", 1.0, 0.0, 0.14, "")).unwrap();
    let d = model.def(&id).unwrap();
    let holes = d.faces.values().filter(|f| f.loops.len() > 1).count();
    assert_eq!(d.kind, "component");
    assert_eq!(d.faces.len(), 6, "one face per letter piece (H e l l o O)");
    assert_eq!(holes, 3, "the e, o and O keep their holes");
    assert!(d.faces.values().all(|f| face_normal(d, f)[2] > 0.99), "all facing up");
    let bb = model.bbox(&id, &cc_m3d::vec::IDENT).unwrap();
    assert!(close(bb.max[1] - bb.min[1], 1.0 + 0.14, 1e-6), "capital height 1 plus the pen width");
    assert!(close(bb.min[0], -0.07, 1e-6) && bb.max[2] == 0.0, "on the ground");
    assert_eq!(d.name, "Hello O#1");
}

#[test]
fn deep_letters_are_closed_solids() {
    let mut model = Model::new();
    let id = model.change("t", |m| text_def(m, "Hi", 0.5, 0.05, 0.14, "")).unwrap();
    let d = model.def(&id).unwrap();
    let si = cc_m3d::solids::solid_info(d);
    assert!(si.closed && si.volume.unwrap_or(0.0) > 0.0, "closed solid letters");
    let bb = model.bbox(&id, &cc_m3d::vec::IDENT).unwrap();
    assert!(close(bb.max[2], 0.05, 1e-6), "5 cm deep");
}

#[test]
fn two_lines() {
    let mut model = Model::new();
    let id = model.change("t", |m| text_def(m, "AB\nCD", 1.0, 0.0, 0.14, "")).unwrap();
    let d = model.def(&id).unwrap();
    let bb = model.bbox(&id, &cc_m3d::vec::IDENT).unwrap();
    assert_eq!(d.faces.len(), 4);
    assert!(close(bb.max[1] - bb.min[1], 1.6 + 1.0 + 0.14, 1e-6), "the second line 1.6 × the height below");
    assert_eq!(d.name, "AB#1");
}

#[test]
fn hostile_text() {
    let mut model = Model::new();
    for t in ["", " ", "\u{0}\u{FFFF}𝕏", "\r\n\r\n", &"W".repeat(400), &"x\n".repeat(80)] {
        let _ = model.change("t", |m| text_def(m, t, 1.0, 0.0, 0.14, ""));
    }
    // solid letters merge their flat sides one edge at a time (as the JS does): short strings only here
    for t in ["", " ", "\u{0}\u{FFFF}𝕏", "\r\n\r\n"] {
        let _ = model.change("t", |m| text_def(m, t, 0.3, 0.1, 0.14, ""));
    }
    let _ = model.change("t", |m| text_def(m, "A", 1e15, 0.0, 0.14, ""));
    let _ = model.change("t", |m| text_def(m, "A", f64::NAN, 0.0, 0.14, ""));
}
