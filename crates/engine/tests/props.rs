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
