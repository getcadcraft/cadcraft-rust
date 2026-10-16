//! Ported from the CadCraft app's own Node test (bin/test-cadcraft.mjs, sections "DXF 2000 save", "Linetypes and
//! lineweights", "Layers frozen in one viewport", "Background masks behind text", "Split screen in files" and
//! tools/cadcraft-test/dwg-save-check.mjs's fixture read back by CadCraft): saveDwg in every version and writeDxf2000,
//! each read back with this crate's importers. The fixture (tests/data/save-fixture.json) is the doc.load form of
//! dwg-save-check.mjs's synthetic drawing: every feature the savers write.

use cc_cad::Doc;
use cc_io_cad::dwg_save::{DWG_VERSIONS, SaveOpts, dwg_version_key, save_dwg};
use serde_json::{Value, json};

fn fixture() -> Value {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/save-fixture.json");
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}
fn load(input: &Value) -> Doc {
    let mut d = Doc::new();
    d.load(input);
    d
}
fn opts(v: &str) -> SaveOpts {
    SaveOpts { version: v.into(), created_ms: Some(1_791_720_000_000.0), guid_sequence: true, ..SaveOpts::default() }
}
/// saveDwg → readDwg → importDwg: the doc.load input of the file read back.
fn dwg_round(d: &Doc, v: &str) -> (Value, Value) {
    let s = save_dwg(d, &opts(v)).expect("saves");
    let tag = String::from_utf8_lossy(&s.bytes[..6]).into_owned();
    let m = cc_io_dwg::read_dwg(&s.bytes, &cc_io_dwg::ReadOpts::default());
    (cc_io_cad::import_dwg(&m, &tag).expect("reads back"), s.report)
}
fn ents(v: &Value) -> Vec<Value> {
    v["entities"].as_array().cloned().unwrap_or_default()
}
fn kinds(v: &Value) -> std::collections::BTreeMap<String, usize> {
    let mut k = std::collections::BTreeMap::new();
    for e in ents(v) {
        *k.entry(e["type"].as_str().unwrap_or("").to_string()).or_insert(0) += 1;
    }
    k
}
fn layer<'a>(v: &'a Value, name: &str) -> &'a Value {
    v["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name)))
        .unwrap_or(&Value::Null)
}
fn notes(r: &Value) -> String {
    r["notes"].as_array().unwrap().iter().map(|n| n.as_str().unwrap()).collect::<Vec<_>>().join(" | ")
}
fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn version_keys() {
    assert_eq!(DWG_VERSIONS.len(), 6);
    assert_eq!(dwg_version_key("2018"), Some("2018"));
    assert_eq!(dwg_version_key("AC1032"), Some("2018"));
    assert_eq!(dwg_version_key("r2004"), Some("2004"));
    assert_eq!(dwg_version_key("ac1015"), Some("2000"));
    assert_eq!(dwg_version_key("R14"), None);
    assert_eq!(dwg_version_key(""), None);
}

#[test]
fn fixture_in_every_version_reads_back() {
    let d = load(&fixture());
    for (key, code) in DWG_VERSIONS {
        let (back, report) = dwg_round(&d, key);
        assert_eq!(report["version"], key);
        let s = save_dwg(&d, &opts(key)).unwrap();
        assert_eq!(&s.bytes[..6], code.as_bytes(), "{key}: the version tag");
        let k = kinds(&back);
        for t in ["line", "circle", "arc", "poly", "ellipse", "text", "insert", "hatch", "dim", "mleader", "xline", "ray", "point"] {
            assert!(k.get(t).copied().unwrap_or(0) >= 1, "{key}: {t} comes back ({k:?})");
        }
        // layers and their states
        assert_eq!(layer(&back, "Off")["on"], false, "{key}: off");
        assert_eq!(layer(&back, "Frozen")["frozen"], true, "{key}: frozen");
        assert_eq!(layer(&back, "Locked")["locked"], true, "{key}: locked");
        assert_eq!(layer(&back, "NoPlot")["plot"], false, "{key}: no plot");
        assert_eq!(layer(&back, "Hidden")["linetype"], "HIDDEN", "{key}: layer linetype");
        assert!(near(layer(&back, "Hidden")["lineweight"].as_f64().unwrap(), 0.35), "{key}: layer lineweight");
        assert_eq!(layer(&back, "Hidden")["newVpFrozen"], true, "{key}: frozen in new viewports");
        // true colours from 2004 on, the nearest standard colour (and a note) in 2000
        let orange = layer(&back, "Orange")["color"].clone();
        if key == "2000" {
            assert!(orange.is_number(), "2000: the nearest index");
            assert!(notes(&report).contains("True colours were saved as the nearest"), "2000: said");
        } else {
            assert_eq!(orange, "#ff7f00", "{key}: a true colour stays");
        }
        // sheets with their viewports; the first one's frozen layer (VP.1) and twist (DR.8)
        let ls = back["layouts"].as_array().unwrap();
        assert_eq!(ls.iter().map(|l| l["name"].as_str().unwrap()).collect::<Vec<_>>(), ["A3 sheet", "Detail"], "{key}: sheets");
        let vp = ls[0]["entities"].as_array().unwrap().iter().find(|e| e["type"] == "viewport").unwrap();
        assert_eq!(vp["frz"], json!(["Hidden"]), "{key}: frozen in the viewport");
        let vp2 = ls[1]["entities"].as_array().unwrap().iter().find(|e| e["type"] == "viewport").unwrap();
        assert!(near(vp2["tw"].as_f64().unwrap(), 0.3), "{key}: the viewport's twist");
        // split screen, UCS, named view, units
        assert_eq!(back["vports"]["layout"], "3r", "{key}: split screen");
        assert!(near(back["ucs"]["current"]["x"].as_f64().unwrap(), 10.0), "{key}: current UCS");
        assert_eq!(back["ucs"]["named"][0]["name"], "Tilted", "{key}: named UCS");
        assert!(back["views"].as_array().unwrap().iter().any(|v| v["name"] == "Kitchen"), "{key}: named view");
        assert_eq!(back["meta"]["units"], "millimeters", "{key}: units");
        // the unloaded link stays a link
        let site = back["blocks"].as_array().unwrap().iter().find(|b| b["name"] == "Site").unwrap();
        assert_eq!(site["xref"]["path"], "X-Refs\\Site.dwg", "{key}: link");
        assert!(notes(&report).contains("saved as a link to its file"), "{key}: the link is said");
        // the clipped reference (XCLIP) and the attribute values
        let refs: Vec<Value> = ents(&back).into_iter().filter(|e| e["type"] == "insert" && e["block"] == "Table 30x72").collect();
        assert!(refs.iter().any(|e| e["clip"]["pts"].as_array().is_some_and(|p| p.len() == 4)), "{key}: the clip comes back");
        assert!(refs.iter().any(|e| e["attrs"]["MAKER"] == "Acme Ø ±"), "{key}: attribute values");
        // the real dimension with its style and override
        let dim = ents(&back).into_iter().find(|e| e["type"] == "dim").unwrap();
        assert_eq!(dim["kind"], "linear", "{key}: linear dimension");
        assert_eq!(dim["dimstyle"], "Arch", "{key}: its style");
        assert_eq!(dim["st"]["pre"], "L=", "{key}: its own prefix");
        // transparency: CADCRAFT keeps it in 2000 (and says other programs cannot)
        if key == "2000" {
            assert!(notes(&report).contains("no transparency"), "2000: said");
        }
    }
}

#[test]
fn per_viewport_overrides_and_masks() {
    let mut fx = fixture();
    // two texts with background masks (TEXTMASK): the drawing's background, and colour 3
    let list = fx["entities"].as_array_mut().unwrap();
    list.push(json!({ "type": "text", "layer": "0", "color": null, "x": 10, "y": 3, "h": 5, "rot": 0, "s": "MASKED", "ha": "left", "va": "baseline", "bgm": { "color": "bg", "scale": 1.5 } }));
    list.push(json!({ "type": "text", "layer": "0", "color": 1, "x": 10, "y": 40, "h": 5, "rot": 0, "s": "RED", "ha": "left", "va": "baseline", "bgm": { "color": 3, "scale": 2 } }));
    let d = load(&fx);
    for v in ["2004", "2018"] {
        let (back, _) = dwg_round(&d, v);
        let masked: Vec<Value> = ents(&back).into_iter().filter(|e| e["bgm"].is_object()).collect();
        assert!(
            masked.iter().any(|e| e["bgm"]["color"] == "bg") && masked.iter().any(|e| e["bgm"]["color"] == 3 && e["bgm"]["scale"] == 2),
            "DWG {v}: masks"
        );
        let vp = back["layouts"][0]["entities"].as_array().unwrap().iter().find(|e| e["type"] == "viewport").cloned().unwrap();
        assert_eq!(vp["lov"]["Walls"]["color"], 5, "DWG {v}: the viewport's layer override (VP.3)");
        assert!(near(vp["lov"]["Walls"]["lw"].as_f64().unwrap(), 0.7));
    }
    let r2000 = save_dwg(&d, &opts("2000")).unwrap().report;
    assert!(notes(&r2000).contains("2 texts have background masks, which the 2000 format cannot hold"), "2000: said");
    let (text, _) = cc_io_cad::dxf2000::write_dxf2000(&d).unwrap();
    let back = cc_io_cad::dxf_read::read_dxf(&text).unwrap();
    assert_eq!(ents(&back).iter().filter(|e| e["bgm"].is_object()).count(), 2, "DXF 2000 keeps both masks");
}

#[test]
fn dxf2000_save() {
    let d = load(&fixture());
    let (text, report) = cc_io_cad::dxf2000::write_dxf2000(&d).unwrap();
    assert!(text.contains("\r\n  1\r\nAC1015\r\n") && text.trim_end().ends_with("EOF"), "AC1015 header, ends in EOF");
    assert!(text.is_ascii(), "plain ASCII (\\U+ for the rest)");
    let back = cc_io_cad::dxf_read::read_dxf(&text).unwrap();
    let k = kinds(&back);
    for t in ["line", "circle", "arc", "ellipse", "text", "insert", "hatch", "dim"] {
        assert!(k.get(t).copied().unwrap_or(0) >= 1, "{t} comes back ({k:?})");
    }
    let ls = back["layouts"].as_array().unwrap();
    assert_eq!(ls.iter().map(|l| l["name"].as_str().unwrap()).collect::<Vec<_>>(), ["A3 sheet", "Detail"]);
    assert!(ls.iter().all(|l| !l["entities"].as_array().unwrap().is_empty()), "every sheet with its objects");
    assert_eq!(layer(&back, "Hidden")["linetype"], "HIDDEN");
    assert!(near(layer(&back, "Hidden")["lineweight"].as_f64().unwrap(), 0.35));
    assert!(
        ents(&back).iter().any(|e| e["type"] == "hatch" && e["patlines"].as_array().is_some_and(|p| !p.is_empty())),
        "pattern lines kept"
    );
    assert!(back["blocks"].as_array().unwrap().iter().any(|b| b["xref"]["path"].is_string()), "an unloaded link stays a link");
    let vp = ls[0]["entities"].as_array().unwrap().iter().find(|e| e["type"] == "viewport").unwrap();
    assert_eq!(vp["frz"], json!(["Hidden"]), "frozen layers by handle (331)");
    let n = notes(&report);
    assert!(n.contains("multileader was saved as the lines") && n.contains("linked drawing"), "{n}");
    // handles are unique and $HANDSEED is above them all
    let lines: Vec<&str> = text.split("\r\n").collect();
    let seed_at = lines.iter().position(|l| *l == "$HANDSEED").unwrap() + 2;
    let mut seen = std::collections::HashSet::new();
    let mut top = 0u64;
    for (k, w) in lines.chunks(2).enumerate() {
        if let [c, v] = w
            && k * 2 + 1 != seed_at
            && matches!(c.trim(), "5" | "105")
            && v.chars().all(|ch| ch.is_ascii_hexdigit())
        {
            assert!(seen.insert(v.to_string()), "handle {v} twice");
            top = top.max(u64::from_str_radix(v, 16).unwrap());
        }
    }
    let seed = lines[seed_at];
    assert!(u64::from_str_radix(seed, 16).unwrap() > top, "HANDSEED {seed} above every handle");
}

#[test]
fn linetypes_and_lineweights() {
    let d = load(&json!({
        "meta": { "units": "millimeters" },
        "layers": [{ "name": "Hidden", "linetype": "HIDDEN", "lineweight": 0.35 }],
        "blocks": [{ "name": "B", "base": { "x": 0, "y": 0 }, "entities": [{ "type": "line", "layer": "0", "lt": "ByBlock", "lw": -2, "x1": 0, "y1": 0, "x2": 100, "y2": 0 }] }],
        "entities": [
            { "type": "line", "layer": "Hidden", "x1": 0, "y1": 0, "x2": 100, "y2": 0 },
            { "type": "insert", "layer": "0", "block": "B", "lt": "DASHED", "lw": 0.7, "x": 0, "y": 50, "sx": 1, "sy": 1, "rot": 0 },
            { "type": "line", "layer": "0", "lt": "DASHED", "lw": 0.5, "lts": 2, "x1": 0, "y1": 20, "x2": 100, "y2": 20 }
        ]
    }));
    for v in ["2000", "2018"] {
        let (back, _) = dwg_round(&d, v);
        let b = load(&back);
        let h = b.layers.get(&"Hidden".to_string()).unwrap();
        assert_eq!(h.linetype.as_deref(), Some("HIDDEN"), "DWG {v}");
        assert_eq!(h.lineweight, Some(0.35), "DWG {v}");
        let e = ents(&back);
        assert!(e[2]["lt"] == "DASHED" && e[2]["lw"] == 0.5 && e[2]["lts"] == 2, "DWG {v}: object linetype, lineweight, scale {:?}", e[2]);
        let bl = back["blocks"].as_array().unwrap().iter().find(|x| x["name"] == "B").unwrap();
        assert!(bl["entities"][0]["lt"] == "ByBlock" && bl["entities"][0]["lw"] == -2, "DWG {v}: by block inside a block");
        let dashed = back["linetypes"].as_array().unwrap().iter().find(|l| l["name"] == "DASHED").unwrap();
        assert!(near(dashed["pattern"][0].as_f64().unwrap(), 12.7), "DWG {v}: the ISO pattern");
    }
}

#[test]
fn saving_is_deterministic_with_pinned_time_and_guids() {
    let d = load(&fixture());
    for v in ["2000", "2007", "2018"] {
        let a = save_dwg(&d, &opts(v)).unwrap();
        let b = save_dwg(&d, &opts(v)).unwrap();
        assert_eq!(a.bytes, b.bytes, "{v}: the same bytes twice");
    }
    let (t1, _) = cc_io_cad::dxf2000::write_dxf2000(&d).unwrap();
    let (t2, _) = cc_io_cad::dxf2000::write_dxf2000(&d).unwrap();
    assert_eq!(t1, t2);
}

#[test]
fn empty_and_odd_drawings_save() {
    // an empty drawing, and one whose entities miss fields or hold the wrong kinds: no panic, a file or an error
    for input in [
        json!({}),
        json!({ "entities": [{ "type": "line" }, { "type": "dim", "kind": "linear" }, { "type": "text", "s": 5 }, { "type": "hatch", "loops": [[1]] }, { "type": "mleader", "pts": [] }] }),
        json!({ "layouts": [{ "name": "", "paper": {} }], "vports": { "layout": "4", "list": [null, 7] }, "ucs": { "current": { "x": "a" } } }),
    ] {
        let d = load(&input);
        for (key, _) in DWG_VERSIONS {
            let _ = save_dwg(&d, &opts(key));
        }
        let _ = cc_io_cad::dxf2000::write_dxf2000(&d);
    }
    let (back, _) = dwg_round(&load(&json!({})), "2018");
    assert!(ents(&back).is_empty());
}
