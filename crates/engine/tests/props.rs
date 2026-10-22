//! The Properties panel's model (M4.11, propdefs.js / propexpr.js): groups, values, one undo step per change, and the
//! number reader (units, feet and inches, angles, expressions), as the 0.5 `bin/test-cadcraft.mjs` "Properties"
//! section checks them. Parity with the frozen JS over many generated edits is `forks/tools/propsdiff.mjs`.

use std::f64::consts::PI;

use cc_cad::Doc;
use cc_cad::entity::Entity;
use cc_engine::Lang;
use cc_engine::props::{PCtx, PVal, ReadCtx, apply_value, defs_for, read_value, value_of};
use serde_json::{Value, json};

static EN: &str = include_str!("../../../assets/lang/cadcraft/en.json");
fn lang() -> Lang {
    Lang::new(&serde_json::from_str::<Value>(EN).unwrap())
}
fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn properties_groups_values_and_one_undo_step_per_change() {
    let l = lang();
    let mut d = Doc::new();
    d.load(&json!({
        "layers": [{"name": "0"}, {"name": "A"}, {"name": "L", "locked": true}],
        "entities": [
            {"id": 1, "type": "line", "layer": "0", "x1": 0, "y1": 0, "x2": 10, "y2": 0},
            {"id": 2, "type": "line", "layer": "A", "x1": 0, "y1": 5, "x2": 3, "y2": 9},
            {"id": 3, "type": "circle", "layer": "L", "cx": 0, "cy": 0, "r": 2}
        ]
    }));
    let ents: Vec<Entity> = [1u64, 2, 3].iter().filter_map(|i| d.get(*i).cloned()).collect();
    let groups = cc_engine::props::groups_of(&ents, &l);
    assert_eq!(groups.iter().map(|g| g.key.as_str()).collect::<Vec<_>>(), ["all", "line", "circle"]);
    let lines: Vec<Entity> = ents.iter().take(2).cloned().collect();
    {
        let c = PCtx { doc: &d, ents: &lines, vertex: 0, lang: &l };
        let defs = defs_for("line", &c);
        let layer = defs.iter().find(|x| x.key == "layer").unwrap();
        assert_eq!(value_of(layer, &c), PVal::Varies);
        let len = defs.iter().find(|x| x.key == "len").unwrap();
        assert_eq!(value_of(len, &c), PVal::Varies);
        assert!(defs.iter().any(|x| x.key == "start.x") && defs.iter().any(|x| x.key == "ang"));
    }
    let r = apply_value(&mut d, "len", "line", &[1, 2], &json!(20), 0, &l);
    assert!(r.ok && r.changed == 2);
    assert_eq!(d.undo_stack.last().map(|s| s.label.as_str()), Some("PROPERTIES"));
    assert_eq!(d.undo_stack.len(), 1, "both lines in one undo step");
    let r = apply_value(&mut d, "layer", "all", &[1, 2, 3], &json!("A"), 0, &l);
    assert!(r.ok && r.changed == 2 && r.locked == 1, "the circle on a locked layer is left alone");
    let r = apply_value(&mut d, "r", "circle", &[3], &json!(-1), 0, &l);
    assert!(r.ok, "set() does not check limits: the reader does");
    let r = apply_value(&mut d, "nosuch", "line", &[1], &json!(1), 0, &l);
    assert!(!r.ok);
}

#[test]
fn number_fields_read_units_feet_inches_angles_and_expressions() {
    let l = lang();
    let mm = ReadCtx { units: "millimeters".into(), ..ReadCtx::default() };
    let arch = ReadCtx { lunits: Some(4.0), units: "inches".into(), ..ReadCtx::default() };
    assert_eq!(read_value("150", "len", &mm, &l), Ok(150.0));
    assert_eq!(read_value("3.5m", "len", &mm, &l), Ok(3500.0));
    assert_eq!(read_value("12'6\"", "len", &arch, &l), Ok(150.0));
    assert_eq!(read_value("=2*45", "ang", &mm, &l), Ok(90.0));
    assert!(near(read_value("45d30'", "ang", &mm, &l).unwrap(), 45.5, 1e-12));
    assert!(near(read_value("0.5r", "ang", &mm, &l).unwrap(), 0.5 * 180.0 / PI, 1e-12));
    assert!(read_value("abc", "len", &mm, &l).is_err());
    assert!(read_value("", "num", &mm, &l).is_err());
    let gt0 = ReadCtx { gt0: true, ..mm.clone() };
    assert!(read_value("0", "len", &gt0, &l).is_err());
    let unitless = ReadCtx::default();
    assert_eq!(read_value("900mm", "len", &unitless, &l), Err(l.s("props.err.nounits")));
    assert!(read_value(&"9".repeat(600), "num", &mm, &l).is_err(), "too long");
}

/// M4.31: a hatch background set to "by block" (0) is dropped as propdefs.js `put` (default 0) drops it; propsdiff
/// found the Rust port keeping `bg: 0`.
#[test]
fn hatch_background_by_block_is_dropped() {
    let l = lang();
    let mut d = Doc::new();
    d.load(&json!({ "entities": [{ "id": 1, "type": "hatch", "layer": "0", "solid": true, "bg": 3, "loops": [[0, 0, 10, 0, 10, 10]] }] }));
    let r = apply_value(&mut d, "bg", "hatch", &[1], &json!(0), 0, &l);
    assert!(r.ok, "{r:?}");
    assert!(d.get(1).unwrap().to_obj().get("bg").is_none());
}

fn door_doc() -> Doc {
    let la = json!({ "type": "line", "layer": "0", "x1": 0, "y1": 0, "x2": 10, "y2": 0 });
    let ar = json!({ "type": "arc", "layer": "0", "cx": 0, "cy": 0, "r": 10, "a0": 0, "a1": 1.6 });
    let lb = json!({ "type": "line", "layer": "0", "x1": 10, "y1": 0, "x2": 10, "y2": 10 });
    let mut d = Doc::new();
    d.load(&json!({
        "blocks": [
            { "name": "Door", "base": { "x": 0, "y": 0 }, "entities": [la, ar],
              "dyn": { "def": "h1", "vis": { "node": 7, "name": "Visibility1", "states": [{ "name": "Open", "show": [0, 1] }, { "name": "Closed", "show": [0, 2] }] },
                       "all": [[la], [ar], [lb]], "shown": [true, true, false], "ad": [], "params": [{ "node": 3, "name": "Width" }] } },
            { "name": "TB", "base": { "x": 0, "y": 0 }, "entities": [la], "attdefs": [{ "tag": "NAME", "def": "", "x": 0, "y": 0, "h": 2 }, { "tag": "K", "def": "k", "constant": true, "x": 0, "y": 0, "h": 2 }] }
        ],
        "entities": [
            { "id": 1, "type": "insert", "layer": "0", "block": "Door", "x": 0, "y": 0, "dyn": { "state": "Open", "values": [{ "node": 7, "kind": "visibility", "state": "Open" }, { "node": 3, "kind": "linear", "distance": 36 }] } },
            { "id": 2, "type": "insert", "layer": "0", "block": "TB", "x": 0, "y": 0, "attrs": { "NAME": "Kitchen" } }
        ]
    }));
    d
}

/// M4.31: Properties › Custom — the visibility state switches the reference to a copy showing that state, in the one
/// PROPERTIES step; the parameter values read back.
#[test]
fn custom_section_switches_visibility_in_one_step() {
    let l = lang();
    let mut d = door_doc();
    let ents = vec![d.get(1).unwrap().clone()];
    {
        let c = PCtx { doc: &d, ents: &ents, vertex: 0, lang: &l };
        let defs = defs_for("insert", &c);
        let vis = defs.iter().find(|x| x.key == "dyn.vis").unwrap();
        assert_eq!(vis.sec, "custom");
        assert_eq!(value_of(vis, &c), PVal::V(json!("Open")));
        let w = defs.iter().find(|x| x.key == "dyn.3").unwrap();
        assert_eq!(w.label.as_deref(), Some("Width"));
        assert_eq!(value_of(w, &c), PVal::V(json!(36)));
    }
    let r = apply_value(&mut d, "dyn.vis", "insert", &[1], &json!("Closed"), 0, &l);
    assert!(r.ok && r.changed == 1, "{r:?}");
    let e = d.get(1).unwrap().to_obj();
    assert_eq!(e.get("block"), Some(&json!("*B1")));
    assert_eq!(e.get("dyn").and_then(|x| x.get("state")), Some(&json!("Closed")));
    assert_eq!(d.undo_stack.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["PROPERTIES"]);
    let bad = apply_value(&mut d, "dyn.vis", "insert", &[1], &json!("Nope"), 0, &l);
    assert!(!bad.ok && d.undo_stack.len() == 1);
}

/// M4.31: the panel's other rows — an attribute (ATTRIBUTE), a constant one refused, the sheet renamed and refused.
#[test]
fn attributes_and_sheet_rows() {
    let l = lang();
    let mut d = door_doc();
    let e = d.get(2).unwrap().clone();
    let (rows, lib) = cc_engine::propextra::attribute_rows(&d, &e);
    assert_eq!(rows.iter().map(|r| (r.tag.as_str(), r.locked)).collect::<Vec<_>>(), [("NAME", false), ("K", true)]);
    assert!(lib.is_none());
    let s = cc_engine::propextra::set_attribute(&mut d, 2, "NAME", "Big\r\nkitchen").unwrap();
    assert_eq!(s.as_deref(), Some("NAME = Big kitchen"));
    assert!(cc_engine::propextra::set_attribute(&mut d, 2, "K", "x").is_err());
    assert_eq!(d.undo_stack.last().map(|s| s.label.as_str()), Some("ATTRIBUTE"));
    let mut d2 = Doc::new();
    d2.load(&json!({ "layouts": [{ "name": "A", "paper": { "key": "iso-a3", "unit": "mm", "w": 420, "h": 297 } }, { "name": "B", "paper": { "w": 10, "h": 5, "unit": "in" } }] }));
    let sp = d2.layouts[0].space;
    d2.set_space_id(sp);
    assert!(cc_engine::propextra::rename_sheet(&mut d2, "B", &l).is_err());
    assert!(cc_engine::propextra::rename_sheet(&mut d2, " A-101 ", &l).unwrap());
    assert_eq!(d2.layouts[0].name, "A-101");
    let m = cc_engine::propextra::set_sheet_paper(&mut d2, "letter", true, &l).unwrap();
    assert!(m.is_some());
}
