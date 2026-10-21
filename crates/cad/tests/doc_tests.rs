//! The Doc behaviours the CadCraft app's Node test relies on (getcadcraft.com/bin/test-cadcraft.mjs, 0.5: 'Blocks',
//! 'Layouts and viewports', undo as one step per command), plus doc.js's own rules (JS Map order on undo, nested
//! changes, rollback on error). Step-by-step parity with the JS Doc: forks/tools/gdiff.mjs (doc-ops script).

use cc_cad::doc::{Block, DocEvent, Layout, MODEL};
use cc_cad::{Doc, DocError, Entity};
use cc_geom::P2;
use serde_json::{Value, json};

fn e(v: Value) -> Entity {
    Entity::from_value(&v)
}
fn line() -> Entity {
    e(json!({ "type": "line", "x1": 0, "y1": 0, "x2": 1, "y2": 1 }))
}
fn types(d: &Doc) -> Vec<String> {
    d.entities().map(|x| x.typ().to_string()).collect()
}

#[test]
fn add_defaults_and_one_step_per_change() {
    let mut d = Doc::new();
    let a = d.change("L", |d| d.add(line())).unwrap();
    assert_eq!(a.id, Some(1));
    assert_eq!(a.layer.as_deref(), Some("0"));
    assert_eq!(a.to_value()["color"], Value::Null);
    d.change("C", |d| {
        d.add(e(json!({ "type": "circle", "layer": "New", "cx": 0, "cy": 0, "r": 1 })))?;
        d.add(line())
    })
    .unwrap();
    assert_eq!(d.undo_stack.len(), 2, "one undo step per change");
    assert!(d.layers.contains_key(&"New".to_string()), "a new layer is made for a new object");
    assert_eq!(d.undo().as_deref(), Some("C"));
    assert_eq!(types(&d), vec!["line"]);
    assert_eq!(d.redo().as_deref(), Some("C"));
    assert_eq!(d.count(), 3);
    assert!(d.change("nothing", |_| Ok(())).is_ok());
    assert_eq!(d.undo_stack.len(), 2, "a change with no edits is no step");
    assert_eq!(d.add(line()).map(|_| ()), Err(DocError::NotInChange));
}

#[test]
fn undone_erase_comes_back_last_as_js_map_order() {
    let mut d = Doc::new();
    d.change("A", |d| {
        for _ in 0..3 {
            d.add(line())?;
        }
        Ok(())
    })
    .unwrap();
    d.change("E", |d| d.remove(1)).unwrap();
    d.undo();
    let ids: Vec<u64> = d.entities().filter_map(|x| x.id).collect();
    assert_eq!(ids, vec![2, 3, 1], "a Map re-set after delete goes to the end");
}

#[test]
fn errors_roll_back_and_nested_changes_join() {
    let mut d = Doc::new();
    let r: Result<(), DocError> = d.change("X", |d| {
        d.add(line())?;
        d.change("inner", |d| d.add(line()).map(|_| ()))?;
        Err(DocError::Failed("no".into()))
    });
    assert!(r.is_err());
    assert_eq!(d.count(), 0);
    assert!(d.undo_stack.is_empty());
    d.change("Y", |d| {
        d.add(line())?;
        d.change("inner", |d| d.add(line()).map(|_| ()))
    })
    .unwrap();
    assert_eq!(d.undo_stack.len(), 1, "a nested change is part of the outer one");
}

#[test]
fn blocks_define_undo_redo_and_uses() {
    let mut d = Doc::new();
    d.change("B", |d| {
        d.define_block(&Block::new(
            "Bolt",
            P2::new(0.0, 0.0),
            vec![line(), e(json!({ "type": "circle", "id": 9, "cx": 0, "cy": 0, "r": 5 }))],
        ))?;
        d.add(e(json!({ "type": "insert", "block": "Bolt", "x": 100, "y": 0, "sx": 2, "rot": std::f64::consts::FRAC_PI_2 })))
    })
    .unwrap();
    assert!(d.blocks.get(&"Bolt".to_string()).unwrap().entities.iter().all(|x| x.id.is_none()), "block entities lose their ids");
    assert_eq!(d.block_uses("Bolt"), 1);
    d.undo();
    assert!(!d.blocks.contains_key(&"Bolt".to_string()) && d.count() == 0, "undo takes the definition away");
    d.redo();
    assert!(d.blocks.contains_key(&"Bolt".to_string()) && d.count() == 1);
    d.change("PURGE", |d| d.remove_block("Nope")).unwrap();
    assert_eq!(d.undo_stack.len(), 1, "removing a block that is not there is no edit");
}

#[test]
fn layouts_undo_across_spaces() {
    let mut d = Doc::new();
    d.change("L", |d| d.add(line())).unwrap();
    let s = d.new_space();
    d.change("LAYOUT", |d| {
        let mut list = d.layouts.clone();
        let mut paper = cc_cad::Obj::new();
        paper.insert("w".into(), json!(420));
        paper.insert("h".into(), json!(297));
        list.push(Layout { name: "Sheet".into(), paper, space: s });
        d.set_layouts(list)
    })
    .unwrap();
    d.set_space(Some("Sheet"));
    assert_eq!(d.drain_events().last(), Some(&DocEvent::Space));
    d.change("VP", |d| d.add(e(json!({ "type": "viewport", "x": 200, "y": 150, "w": 100, "h": 80, "cx": 0, "cy": 0, "s": 1 })))).unwrap();
    assert!(d.layout().is_some() && d.count() == 1 && d.space_map(MODEL).unwrap().len() == 1, "drawing on a sheet draws in paper space");
    d.set_space(None);
    d.undo();
    assert_eq!(d.space_map(s).unwrap().len(), 0, "the sheet's step undoes on the sheet while the model is open");
    d.set_space(Some("Sheet"));
    d.undo();
    assert!(d.layouts.is_empty() && d.in_model(), "undoing LAYOUT removes the sheet and goes back to the model");
    assert!(d.drain_events().contains(&DocEvent::Space));
    d.redo();
    assert_eq!(d.layouts.len(), 1);
}

#[test]
fn mark_squash_and_rename_layer() {
    let mut d = Doc::new();
    let m = d.mark();
    for _ in 0..3 {
        d.change("L", |d| d.add(e(json!({ "type": "line", "layer": "Old", "x1": 0, "y1": 0, "x2": 1, "y2": 0 })))).unwrap();
    }
    d.squash(m, "LINES");
    assert_eq!(d.undo_stack.len(), 1);
    assert_eq!(d.undo_stack[0].label, "LINES");
    let mut patch = cc_cad::Obj::new();
    patch.insert("name".into(), json!("New"));
    d.set_layer("Old", &patch, true).unwrap();
    assert!(
        d.entities().all(|x| x.layer.as_deref() == Some("New")) && d.current.layer == "New",
        "a new name moves the objects on the layer"
    );
    assert_eq!(d.undo_stack.last().unwrap().label, "RENAME LAYER");
    d.undo();
    assert!(d.entities().all(|x| x.layer.as_deref() == Some("Old")) && d.current.layer == "0");
}

#[test]
fn load_checks_file_data() {
    let mut d = Doc::new();
    d.load(&json!({
        "entities": [{ "type": "line", "x1": 0, "y1": 0, "x2": 1, "y2": 0, "ccref": 3, "layer": "" }, { "type": "face3d", "pts": [] }],
        "blocks": [{ "name": "CADCRAFT_3D", "entities": [] }, { "name": "K", "entities": [], "xref": { "path": 12 } }],
        "layouts": [{ "name": "S", "paper": { "w": 10 }, "entities": [{ "type": "point", "x": 1, "y": 1 }] }],
        "views": [{ "name": "V", "cx": 0, "cy": 0, "w": 1, "h": 1 }, { "name": "", "cx": 0, "cy": 0, "w": 1, "h": 1 }],
        "linetypes": [{ "name": "OK", "pattern": [1, -1] }, { "name": "BAD", "pattern": [1, "x"] }],
        "textStyles": [["T", { "font": "Arial", "h": -2, "wf": 0, "obl": 0.1 }]],
        "viewTwist": 1000
    }));
    let s = d.snapshot();
    assert_eq!(s["model"].as_array().unwrap().len(), 1, "face3d left out of the drawing");
    assert!(s["model"][0].get("ccref").is_none() && s["model"][0]["layer"] == "0" && s["model"][0]["color"] == Value::Null);
    assert_eq!(s["blocks"].as_array().unwrap().len(), 1, "the 3D model's block is not a drawing block");
    assert_eq!(s["blocks"][0]["xref"], json!({ "path": "12" }));
    assert_eq!(s["layouts"][0]["paper"], json!({ "x0": 0, "y0": 0, "w": 10 }));
    assert_eq!(s["layouts"][0]["entities"][0]["id"], json!(2));
    assert_eq!(s["views"].as_array().unwrap().len(), 1);
    assert_eq!(s["linetypes"].as_array().unwrap().len(), 1);
    assert_eq!(s["textStyles"][0][1], json!({ "font": "Arial", "h": 0, "wf": 1, "obl": 0.1 }));
    assert_eq!(s["viewTwist"], json!(0));
    assert!(!d.dirty() && d.undo_stack.is_empty());
}

/// M4.14: an object's extra keys (xdata, unknown codes) are shared copy-on-write, as the JS shares nested objects:
/// a move, its undo step and an undo clone none of them; a write to one copy leaves the others as they were.
#[test]
fn extra_keys_shared_through_edits_and_undo() {
    let xd = json!({ "ACAD": [[1000, "a long xdata string"], [1040, 2.5]], "APP": { "k": [1, 2, 3] } });
    let mut d = Doc::new();
    let a = d.change("L", |d| d.add(e(json!({ "type": "line", "x1": 0, "y1": 0, "x2": 1, "y2": 1, "xd": xd, "lw": 0.25 })))).unwrap();
    let id = a.id.unwrap();
    let stored = d.get(id).unwrap().extra.clone();
    let mut moved = d.get(id).unwrap().clone();
    assert!(cc_cad::entity::Extra::ptr_eq(&moved.extra, &stored), "a clone shares the extra keys");
    if let cc_cad::Geo::Line(l) = &mut moved.geo {
        l.x1 = 5.0;
    }
    assert!(d.change("MOVE", |d| d.update(id, moved)).unwrap());
    assert!(cc_cad::entity::Extra::ptr_eq(&d.get(id).unwrap().extra, &stored), "the edit kept the same map");
    // removing a key that is not there copies nothing (every add / update removes "id")
    let mut c = d.get(id).unwrap().clone();
    assert!(c.extra.remove("nope").is_none());
    assert!(cc_cad::entity::Extra::ptr_eq(&c.extra, &stored));
    // a write copies: the document keeps its own
    c.extra.insert("tag".into(), json!("T"));
    assert!(!cc_cad::entity::Extra::ptr_eq(&c.extra, &stored));
    assert!(d.get(id).unwrap().extra.get("tag").is_none());
    d.undo();
    assert_eq!(d.get(id).unwrap().to_value()["x1"], json!(0));
    assert_eq!(d.get(id).unwrap().to_value()["xd"], xd);
    assert!(cc_cad::entity::Extra::ptr_eq(&d.get(id).unwrap().extra, &stored), "undo restores without a copy");
    d.redo();
    assert_eq!(d.get(id).unwrap().to_value()["x1"], json!(5));
    assert_eq!(d.get(id).unwrap().to_value()["xd"], xd);
    // updating an id that is not there changes nothing
    assert!(!d.change("X", |d| d.update(999, line())).unwrap());
    assert_eq!(d.undo_stack.len(), 2);
}

/// commands.js's VIEWLABEL edit hook: a view label follows its viewport's view name in the same undo step.
#[test]
fn view_labels_follow_their_viewport_name() {
    let mut d = Doc::new();
    let vp = d
        .change("MVIEW", |d| d.add(e(json!({ "type": "viewport", "x": 50, "y": 50, "w": 80, "h": 60, "cx": 0, "cy": 0, "s": 1 }))))
        .unwrap();
    let vid = vp.id.unwrap();
    let label = d
        .change("VIEWLABEL", |d| {
            d.add(e(json!({ "type": "text", "x": 10, "y": 10, "h": 2.5, "s": "Unnamed view", "field": { "k": "vpview", "vp": vid } })))
        })
        .unwrap();
    let lid = label.id.unwrap();
    let other = d.change("TEXT", |d| d.add(e(json!({ "type": "text", "x": 0, "y": 0, "h": 2.5, "s": "Plain" })))).unwrap().id.unwrap();
    let mut named = d.get(vid).unwrap().clone();
    named.extra.insert("vname".into(), json!("Plan"));
    d.change("VIEW NAME", |d| d.update(vid, named)).unwrap();
    assert_eq!(d.get(lid).unwrap().to_value()["s"], json!("Plan"));
    assert_eq!(d.get(other).unwrap().to_value()["s"], json!("Plain"));
    assert_eq!(d.undo_stack.last().unwrap().ops.len(), 2, "the label changed in the same step");
    d.undo();
    assert_eq!(d.get(lid).unwrap().to_value()["s"], json!("Unnamed view"));
    // a name taken away: the label says so; the viewport erased: the label keeps its words
    d.redo();
    let mut plain = d.get(vid).unwrap().clone();
    plain.extra.remove("vname");
    d.change("VIEW NAME", |d| d.update(vid, plain)).unwrap();
    assert_eq!(d.get(lid).unwrap().to_value()["s"], json!("Unnamed view"));
    d.change("ERASE", |d| d.remove(vid)).unwrap();
    assert_eq!(d.get(lid).unwrap().to_value()["s"], json!("Unnamed view"));
}

/// S.4: a block's objects are shared copy-on-write between the drawing and its undo step (a linked drawing of 21k
/// objects is not copied for undo); a later edit of the block copies, and undo / redo give each version back.
#[test]
fn block_objects_shared_copy_on_write() {
    let mut d = Doc::new();
    let b = Block::new("B", P2::new(0.0, 0.0), vec![line(), line(), line()]);
    d.change("BLOCK", |d| d.define_block_owned(b)).unwrap();
    let held = d.blocks.get(&"B".to_string()).unwrap().entities.clone();
    let in_step = match d.undo_stack.last().and_then(|s| s.ops.last()) {
        Some(cc_cad::doc::Op::Block { after: Some(a), .. }) => a.entities.clone(),
        _ => panic!("a block op"),
    };
    assert!(cc_cad::Ents::ptr_eq(&held, &in_step), "the step shares the list");
    let mut b2 = d.blocks.get(&"B".to_string()).unwrap().clone();
    assert!(cc_cad::Ents::ptr_eq(&b2.entities, &held), "a clone shares the list");
    b2.entities.push(line());
    assert!(!cc_cad::Ents::ptr_eq(&b2.entities, &held), "a write copies");
    assert_eq!(held.len(), 3);
    d.change("BEDIT", |d| d.define_block(&b2).map(|_| ())).unwrap();
    assert_eq!(d.blocks.get(&"B".to_string()).unwrap().entities.len(), 4);
    d.undo();
    assert_eq!(d.blocks.get(&"B".to_string()).unwrap().entities.len(), 3);
    d.redo();
    assert_eq!(d.blocks.get(&"B".to_string()).unwrap().entities.len(), 4);
    d.undo();
    d.undo();
    assert!(d.blocks.get(&"B".to_string()).is_none());
}
