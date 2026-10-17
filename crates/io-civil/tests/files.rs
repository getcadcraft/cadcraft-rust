//! cc-io-civil's writers and readers: LandXML out and back in, DXF group pairs, the PDF's structure, DWG bytes that do
//! not change between runs, the GLB container. Fixtures are generated here.

use serde_json::{Value, json};

fn run(calls: Value) -> Vec<Value> {
    let out = cc_civil::script::run_with(&json!({ "calls": calls }), &[cc_io_civil::script::handle, cc_civil::script::handle]);
    out.as_array().cloned().unwrap_or_default()
}
fn ok(v: &Value) -> &Value {
    assert!(v.get("err").is_none(), "call failed: {v}");
    &v["ok"]
}
fn b64(v: &Value) -> Vec<u8> {
    let s = v["$base64"].as_str().unwrap_or("");
    cc_civil::project::unb64(s).unwrap_or_default()
}
fn site() -> (Vec<Value>, f64, f64) {
    let (ox, oy) = (3_115_000.5, 10_070_000.25);
    let pts = (0..120)
        .map(|i| {
            let (u, v) = (f64::from(i % 12) * 25.0 + 0.3, f64::from(i / 12) * 25.0 + 0.7);
            json!({ "n": (i + 1).to_string(), "x": ox + u, "y": oy + v, "z": 100.0 + (u / 50.0).sin() * 4.0 + v * 0.01, "d": "GRD" })
        })
        .collect();
    (pts, ox, oy)
}

#[test]
fn landxml_round_trip() {
    let (pts, ox, oy) = site();
    let r = run(json!([
        { "op": "tin.buildSurface", "args": [{ "points": pts }], "as": "S" },
        { "op": "alignment.build", "args": [{ "pis": [{ "x": ox + 10.0, "y": oy + 10.0 }, { "x": ox + 150.0, "y": oy + 40.0, "r": 80 }, { "x": ox + 280.0, "y": oy + 200.0 }], "start": 1000 }], "as": "A" },
        { "op": "lx.write", "args": [{ "name": "T & <x>", "units": "ft", "points": pts, "surfaces": [{ "name": "EG", "surface": "S", "breaklines": [], "boundary": null }], "alignments": [{ "name": "CL", "alignment": "A" }], "date": 0 }], "as": "X" },
        { "op": "lx.read", "args": [{ "ref": "X" }] },
    ]));
    let d = ok(&r[3]);
    let back = d["points"].as_array().cloned().unwrap_or_default();
    assert_eq!(back.len(), pts.len());
    for (a, b) in pts.iter().zip(back.iter()) {
        assert_eq!(a["n"], b["n"]);
        assert!((a["x"].as_f64().unwrap_or(0.0) - b["x"].as_f64().unwrap_or(1.0)).abs() < 1e-6, "survey x kept");
    }
    assert_eq!(d["surfaces"].as_array().map(Vec::len), Some(1));
    assert_eq!(d["alignments"][0]["start"].as_f64(), Some(1000.0));
}

#[test]
fn dxf_pdf_dwg_glb() {
    let (pts, _, _) = site();
    let spec = json!({ "units": "ft", "points": pts, "surface": "S", "contours": { "interval": 1, "major": 5 }, "breaklines": [], "alignments": [], "pads": [] });
    let mm = 25.4 / 40.0;
    let r = run(json!([
        { "op": "tin.buildSurface", "args": [{ "points": pts }], "as": "S" },
        { "op": "scene.plan", "args": [spec, { "mmPerUnit": mm }], "as": "SC" },
        { "op": "dxf.fromScene", "args": ["SC", mm, "ft"] },
        { "op": "dwg.fromScene", "args": [{ "scene": "SC", "mmPerUnit": mm, "units": "ft", "version": "AC1032", "created": 1.7e12 }] },
        { "op": "dwg.fromScene", "args": [{ "scene": "SC", "mmPerUnit": mm, "units": "ft", "version": "AC1032", "created": 1.7e12 }] },
        { "op": "project.new", "as": "P" },
        { "op": "project.setPoints", "self": "P", "args": [pts, "a.csv"] },
        { "op": "s3.input", "args": ["P"], "as": "I" },
        { "op": "s3.build", "args": ["I", { "show": { "points": true } }], "as": "G" },
        { "op": "glb.export", "args": ["G", { "name": "Site" }] },
        { "op": "obj.export", "args": ["G"] },
    ]));
    // DXF: code / value pairs, CRLF, ends with EOF
    let text = ok(&r[2])["text"].as_str().unwrap_or("").to_string();
    let lines: Vec<&str> = text.split("\r\n").collect();
    for pair in lines.chunks(2) {
        if pair.len() == 2 {
            assert!(pair[0].trim().parse::<i32>().is_ok(), "group code: {:?}", pair[0]);
        }
    }
    assert!(text.contains("EOF"));
    // DWG: AC1032 magic, same bytes twice (deterministic handles and GUIDs)
    let d1 = b64(&ok(&r[3])["bytes"]);
    let d2 = b64(&ok(&r[4])["bytes"]);
    assert!(d1.starts_with(b"AC1032"));
    assert_eq!(d1, d2, "the DWG writer is deterministic");
    // GLB: glTF magic, version 2, total length, 4-byte padding
    let g = b64(ok(&r[9]));
    assert_eq!(&g[0..4], b"glTF");
    assert_eq!(u32::from_le_bytes([g[4], g[5], g[6], g[7]]), 2);
    assert_eq!(u32::from_le_bytes([g[8], g[9], g[10], g[11]]) as usize, g.len());
    assert_eq!(g.len() % 4, 0);
    let obj = ok(&r[10]).as_str().unwrap_or("");
    assert!(obj.starts_with("# CivilCraft site") && obj.contains("\nv ") && obj.contains("\nf "));
}

#[test]
fn pdf_structure() {
    let (pts, ox, oy) = site();
    let spec = json!({ "units": "ft", "points": pts, "surface": "S", "contours": { "interval": 1, "major": 5 }, "breaklines": [], "alignments": [], "pads": [] });
    let scale = json!({ "label": "1\" = 40'", "mmPerUnit": 25.4 / 40.0 });
    let ext = json!({ "minX": ox, "minY": oy, "maxX": ox + 300.0, "maxY": oy + 250.0 });
    let r = run(json!([
        { "op": "tin.buildSurface", "args": [{ "points": pts }], "as": "S" },
        { "op": "scene.plan", "args": [spec, { "mmPerUnit": 25.4 / 40.0 }], "as": "SC" },
        { "op": "plot.planSheet", "args": ["SC", { "extents": ext, "paper": "tabloid", "scale": scale, "title": "Plan", "units": "ft" }], "as": "SH" },
        { "op": "plot.toPdf", "args": ["SH", {}] },
    ]));
    let pdf = ok(&r[3]);
    let view = pdf["view"].as_array().cloned().unwrap_or_default();
    assert!(view.iter().any(|o| o.as_str().is_some_and(|s| s.contains("/Type /Catalog"))));
    assert!(
        view.iter().any(|o| o.as_str().is_some_and(|s| s.contains("|flate|") && s.contains(" l"))),
        "a deflated page stream with linework"
    );
    assert_eq!(pdf["report"]["missing"], json!(0));
}

#[test]
fn hostile_files_do_not_panic() {
    let texts = [
        "",
        "<",
        "<LandXML>",
        "<LandXML><Surfaces><Surface><Definition><Pnts><P id=\"a\">1 2</P></Pnts><Faces><F>a a a</F></Faces></Definition></Surface></Surfaces></LandXML>",
        "<LandXML><CgPoints><CgPoint>1e999 -1e999 NaN</CgPoint></CgPoints></LandXML>",
        "<!DOCTYPE a [<!ENTITY x \"&x;&x;\">]><LandXML>&x;</LandXML>",
    ];
    for t in texts {
        let _ = run(
            json!([{ "op": "lx.read", "args": [t] }, { "op": "xml.parse", "args": [t] }, { "op": "lx.scaleImport", "args": [t, 0.3048] }]),
        );
    }
    let deep = "<a>".repeat(5000);
    let _ = run(json!([{ "op": "xml.parse", "args": [deep] }]));
}
