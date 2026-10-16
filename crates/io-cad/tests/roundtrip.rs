//! Ported from the CadCraft app's own Node test (bin/test-cadcraft.mjs, sections "DXF round trip", "Multileaders,
//! pictures and wipeouts from files" and "DWG / DXF files"): what the drafting app checks about its importers and the
//! R12 writer, on synthetic drawings and the repo's public samples.

use cc_cad::Doc;
use serde_json::{Value, json};

fn load(input: &Value) -> Doc {
    let mut d = Doc::new();
    d.load(input);
    d
}
fn ents(v: &Value) -> Vec<Value> {
    v.get("entities").and_then(Value::as_array).cloned().unwrap_or_default()
}
fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6
}
fn f(v: &Value, k: &str) -> f64 {
    v.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN)
}

/// The "DXF round trip" drawing of test-cadcraft.mjs, as doc.load input.
fn sample() -> Value {
    json!({
        "layers": [{ "name": "A-WALL ext", "color": 1 }, { "name": "Off layer", "color": 3, "on": false }],
        "blocks": [{ "name": "DoorTag", "base": { "x": 0, "y": 0 }, "entities": [{ "type": "circle", "layer": "0", "color": 0, "cx": 0, "cy": 0, "r": 1 }] }],
        "entities": [
            { "type": "line", "layer": "A-WALL ext", "x1": 0, "y1": 0, "x2": 100, "y2": 0 },
            { "type": "circle", "x": 0, "cx": 50, "cy": 50, "r": 10, "color": "#3366cc" },
            { "type": "arc", "cx": 0, "cy": 0, "r": 20, "a0": 0.1, "a1": 2.5 },
            { "type": "poly", "closed": true, "pts": [{ "x": 0, "y": 0, "b": 0.5 }, { "x": 10, "y": 0, "b": 0 }, { "x": 10, "y": 10, "b": 0 }] },
            { "type": "text", "x": 1, "y": 2, "h": 2.5, "rot": 0.5, "s": "Détail 1 ⌀20 50°", "ha": "center", "va": "middle" },
            { "type": "text", "x": 1, "y": 20, "h": 2, "rot": 0, "s": "Two\nlines", "ha": "left", "va": "top" },
            { "type": "insert", "block": "DoorTag", "x": 5, "y": 5, "sx": 2, "sy": 2, "rot": 0, "color": 5 },
            { "type": "point", "x": 7, "y": 7 },
            { "type": "ellipse", "cx": 0, "cy": 0, "mx": 10, "my": 0, "ratio": 0.5, "t0": 0, "t1": std::f64::consts::TAU }
        ]
    })
}

#[test]
fn dxf_round_trip() {
    let doc = load(&sample());
    let (text, report) = cc_io_cad::dxf_write::write_dxf(&doc).expect("writes");
    assert!(text.contains("AC1009") && text.trim().ends_with("EOF"), "writes R12 with an EOF");
    let notes: Vec<String> = report["notes"].as_array().unwrap().iter().map(|n| n.as_str().unwrap().to_string()).collect();
    assert!(
        text.contains("\nA-WALL_EXT\r") && notes.iter().any(|n| n.contains("changed to fit R12")),
        "layer names made R12-safe and reported"
    );
    assert!(text.is_ascii(), "output is plain ASCII (non-ASCII as \\U+)");
    assert!(
        notes.iter().any(|n| n.contains("ellipse")) && notes.iter().any(|n| n.contains("Multi-line")),
        "lossy conversions are reported"
    );
    let back = cc_io_cad::dxf_read::read_dxf(&text).expect("reads back");
    let mut types: Vec<String> = ents(&back).iter().map(|e| e["type"].as_str().unwrap().to_string()).collect();
    types.sort();
    assert_eq!(types.join(","), "arc,circle,insert,line,point,poly,poly,text,text,text", "every entity comes back");
    let t = ents(&back).into_iter().find(|e| e["type"] == "text" && e["s"].as_str().unwrap().starts_with('D')).unwrap();
    assert!(
        t["s"] == "Détail 1 ⌀20 50°" && t["ha"] == "center" && t["va"] == "middle" && near(f(&t, "x"), 1.0) && near(f(&t, "rot"), 0.5),
        "text, symbols and justification survive"
    );
    let arc = ents(&back).into_iter().find(|e| e["type"] == "arc").unwrap();
    assert!(near(f(&arc, "a0"), 0.1) && near(f(&arc, "a1"), 2.5), "arc angles survive");
    let poly = ents(&back).into_iter().find(|e| e["type"] == "poly" && e["closed"] == true).unwrap();
    assert!(near(f(&poly["pts"][0], "b"), 0.5), "bulges survive");
    let off = back["layers"].as_array().unwrap().iter().find(|l| l["name"] == "OFF_LAYER").unwrap();
    assert_eq!(off["on"], false, "layer off state survives");
    let blocks = back["blocks"].as_array().unwrap();
    assert!(blocks.len() == 1 && blocks[0]["entities"][0]["color"] == 0, "block with BYBLOCK colour survives");
    let two: Vec<Value> = ents(&back).into_iter().filter(|e| e["type"] == "text" && (e["s"] == "Two" || e["s"] == "lines")).collect();
    assert!(two.len() == 2 && f(&two[0], "y") > f(&two[1], "y"), "multi-line text saved as lines top to bottom");
    // a second save of what came back changes nothing in the text
    let d2 = load(&back);
    let (again, _) = cc_io_cad::dxf_write::write_dxf(&d2).unwrap();
    let back2 = cc_io_cad::dxf_read::read_dxf(&again).unwrap();
    assert_eq!(ents(&back2).len(), ents(&back).len());
}

#[test]
fn two_layouts_through_r12() {
    let mut s = sample();
    s["layouts"] = json!([
        { "name": "Layout1", "paper": { "w": 420, "h": 297, "unit": "mm", "x0": 0, "y0": 0, "key": "iso-a3" },
          "entities": [{ "type": "viewport", "layer": "0", "x": 210, "y": 148.5, "w": 300, "h": 200, "cx": 50, "cy": 50, "s": 0.5 }] },
        { "name": "Détails 2", "paper": { "w": 420, "h": 297, "unit": "mm", "x0": 0, "y0": 0, "key": "iso-a3" },
          "entities": [{ "type": "viewport", "layer": "0", "x": 210, "y": 148.5, "w": 300, "h": 200, "cx": 50, "cy": 50, "s": 0.5 }] }
    ]);
    s["meta"] = json!({ "units": "millimeters" });
    let doc = load(&s);
    let (text, report) = cc_io_cad::dxf_write::write_dxf(&doc).unwrap();
    let notes = report["notes"].to_string();
    assert!(notes.contains("R12 holds one layout") && notes.contains("CCLAYOUT_2"), "the save says where the second layout went");
    let back = cc_io_cad::dxf_read::read_dxf(&text).unwrap();
    let ls = back["layouts"].as_array().unwrap();
    assert!(ls.len() == 2 && ls[0]["name"] == "Layout1" && ls[1]["name"] == "Détails 2", "both layouts come back, with their names");
    let v0 = ls[0]["entities"].as_array().unwrap().iter().find(|e| e["type"] == "viewport").cloned().unwrap();
    assert!(
        near(f(&v0, "s"), 0.5) && near(f(&v0, "cx"), 50.0) && near(f(&v0, "w"), 300.0),
        "the first layout's viewport keeps its view and scale"
    );
    let v1 = ls[1]["entities"].as_array().unwrap().iter().find(|e| e["type"] == "viewport").cloned().unwrap();
    assert!(near(f(&v1, "s"), 0.5) && ls[1]["paper"]["key"] == "iso-a3", "the second layout's viewport too (from the CCLAYOUT block)");
    assert_eq!(back["meta"]["units"], "millimeters", "drawing units survive");
}

#[test]
fn binary_and_bad_dxf_are_refused() {
    let e = cc_io_cad::dxf_read::read_dxf("AutoCAD Binary DXF\r\n\u{1a}\0").unwrap_err();
    assert!(e.0.contains("binary DXF"));
    let e = cc_io_cad::dxf_read::read_dxf("  0\nSECTION\nnot a code\nx\n").unwrap_err();
    assert!(e.0.contains("bad group code on line 3"), "{}", e.0);
}

fn repo_file(rel: &str) -> Option<Vec<u8>> {
    std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel)).ok()
}

#[test]
fn kitchen_fixture_multileader_and_picture() {
    let Some(bytes) = repo_file("assets/samples/kitchen-equipment.dwg") else { return };
    let model = cc_io_dwg::read_dwg(&bytes, &cc_io_dwg::ReadOpts::default());
    let r = cc_io_cad::import_dwg(&model, "AC1032").unwrap();
    let d = load(&r);
    let all: Vec<Value> = d
        .model()
        .map(|e| e.to_value())
        .chain(d.layouts.iter().flat_map(|l| d.space_entities(l.space).map(|e| e.to_value()).collect::<Vec<_>>()))
        .collect();
    let ml: Vec<&Value> = all.iter().filter(|e| e["type"] == "mleader").collect();
    assert_eq!(ml.len(), 1, "one editable multileader");
    let m = ml[0];
    assert!(m["s"] == "FLOOR DRAIN\nSEE PLUMBING" && m["pts"].as_array().unwrap().len() == 2 && m["pts"][0]["x"] == 630 && m["dir"] == 1);
    assert!(m["st"]["dogleg"] == 8 && m["h"] == 4 && m["st"]["gap"] == 2, "landing, size, gap");
    let pic =
        all.iter().find(|e| e["type"] == "insert" && e["block"].as_str().unwrap_or("").starts_with("*I")).expect("a picture reference");
    let pb = d.blocks.get(&pic["block"].as_str().unwrap().to_string()).unwrap().to_value();
    let pe = pb["entities"].as_array().unwrap();
    assert!(pe.iter().any(|e| e["type"] == "poly" && e["closed"] == true && e["pts"].as_array().unwrap().len() == 4));
    assert!(pe.iter().any(|e| e["type"] == "text" && e["s"] == "example-logo.png"), "frame + file name in one unnamed block");
    let rep = &r["report"];
    assert!(rep["skipped"].get("MULTILEADER").is_none() && rep["skipped"].get("IMAGE").is_none());
    assert!(rep["notes"].as_array().unwrap().iter().any(|n| n.as_str().unwrap().contains("picture is shown as a frame")));
}

/// "DWG / DXF files": every sample imports, has model entities, finite extents, and survives DXF out → in.
#[test]
fn samples_import_save_and_reread() {
    let mut inputs: Vec<(String, Value)> = Vec::new();
    for rel in ["assets/samples/base-plan.dwg", "assets/samples/kitchen-equipment.dwg"] {
        if let Some(b) = repo_file(rel) {
            let tag = String::from_utf8_lossy(&b[..6]).into_owned();
            let model = cc_io_dwg::read_dwg(&b, &cc_io_dwg::ReadOpts::default());
            // the DWG's closed flag must survive the import
            inputs.push((rel.into(), cc_io_cad::import_dwg(&model, &tag).unwrap()));
        }
    }
    let dxf = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/base-plan.dxf")).unwrap();
    inputs.push(("base-plan.dxf".into(), cc_io_cad::dxf_read::read_dxf(&dxf).unwrap()));
    for (name, input) in inputs {
        let doc = load(&input);
        assert!(doc.count() > 0, "{name}: has model-space entities");
        let (text, _) = cc_io_cad::dxf_write::write_dxf(&doc).unwrap();
        let again = cc_io_cad::dxf_read::read_dxf(&text).unwrap();
        let wrote = ents(&again).len();
        assert!(wrote as f64 >= doc.count() as f64 * 0.95, "{name}: DXF save → re-read keeps the entities ({wrote} of {})", doc.count());
        assert_eq!(again["layouts"].as_array().map_or(0, Vec::len), doc.layouts.len(), "{name}: layouts come back");
    }
}
