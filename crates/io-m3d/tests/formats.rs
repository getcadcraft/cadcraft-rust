//! The format sections of ModelCraft's Node test (bin/test-modelcraft.mjs → tools/modelcraft-test/formats.mjs at the
//! frozen 0.5 commit), ported: every check that does not need an outside reader (trimesh, web-ifc, ezdxf, pycollada,
//! the Khronos validator). Fixtures are built here; no corpus or client file is read.

use cc_io_m3d::{cad, collada, gltf, ifc, native, obj, site, stl};
use cc_m3d::def::{face_area, face_normal};
use cc_m3d::model::place;
use cc_m3d::ops::{add_polygon, push_pull, rect_pts};
use cc_m3d::vec::{IDENT, M4, P3, dot, mmul, rotation, scaling, translation};
use cc_m3d::{Def, Model};
use serde_json::{Value, json};

const DATE: &str = "2026-10-10T12:00:00.000Z";

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn faces_of(d: &Def) -> Vec<f64> {
    d.faces.keys().collect()
}

/// The format test's model: a 30 m³ loose box, a component "Column" placed three times (one scaled), a group "Wall with
/// opening", an open "Pad", a loose edge.
struct Fx {
    m: Model,
    col: String,
    wall: String,
}
fn formats_model() -> Fx {
    let mut m = Model::new();
    {
        let root = m.root_mut().unwrap();
        add_polygon(root, &rect_pts([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 4.0, 3.0), None).unwrap();
        let f = faces_of(root)[0];
        push_pull(root, f, 2.5);
    }
    let col = m.new_def("component", Some("Column"));
    {
        let d = m.def_mut(&col).unwrap();
        add_polygon(d, &rect_pts([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0.4, 0.4), None).unwrap();
        let f = faces_of(d)[0];
        push_pull(d, f, 3.0);
    }
    let placements: [M4; 3] = [
        translation([6.0, 0.0, 0.0]),
        mmul(&translation([8.0, 2.0, 0.0]), &rotation([0.0, 0.0, 1.0], std::f64::consts::PI / 6.0, [0.0; 3])),
        mmul(&translation([10.0, 0.0, 0.0]), &scaling(1.5, [0.0; 3])),
    ];
    for pm in placements {
        place(m.root_mut().unwrap(), &col, &pm);
    }
    let wall = m.new_def("group", Some("Wall with opening"));
    {
        let d = m.def_mut(&wall).unwrap();
        add_polygon(d, &rect_pts([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 4.0, 0.3), None).unwrap();
        let f = faces_of(d)[0];
        push_pull(d, f, 3.0);
        add_polygon(d, &rect_pts([1.0, 0.0, 0.5], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0, 1.5), None).unwrap();
        let win = d
            .faces
            .iter()
            .find(|(_, f)| dot(face_normal(d, f), [0.0, -1.0, 0.0]) > 0.999 && close(face_area(d, f), 1.5, 1e-6))
            .map(|(id, _)| id)
            .unwrap();
        push_pull(d, win, -5.0);
    }
    place(m.root_mut().unwrap(), &wall, &translation([0.0, 6.0, 0.0]));
    let pad = m.new_def("group", Some("Pad"));
    add_polygon(m.def_mut(&pad).unwrap(), &rect_pts([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 2.0, 2.0), None).unwrap();
    place(m.root_mut().unwrap(), &pad, &translation([0.0, 12.0, 0.0]));
    cc_m3d::edges::add_edge(m.root_mut().unwrap(), [14.0, 10.0, 0.0], [16.0, 10.0, 0.0], Default::default());
    Fx { m, col, wall }
}

fn def_area(d: &Def) -> f64 {
    d.faces.values().map(|f| face_area(d, f)).sum()
}
fn world_area(m: &Model) -> f64 {
    cc_m3d::mesh::world_triangles(m, "root", &IDENT)
        .iter()
        .map(|t| cc_m3d::vec::len(cc_m3d::vec::cross(cc_m3d::vec::sub(t.p[1], t.p[0]), cc_m3d::vec::sub(t.p[2], t.p[0]))) / 2.0)
        .sum()
}

#[test]
fn native_file_round_trips() {
    let mut fx = formats_model();
    let text = native::export_native(&mut fx.m);
    let j: Value = serde_json::from_str(&text).unwrap();
    let mut m2 = native::load_native(&j).unwrap();
    assert_eq!(m2.snapshot(), fx.m.snapshot(), "save → open gives the identical model");
    assert_eq!(m2.instance_counts().get(&fx.col).copied(), Some(3.0));
    // refused: no format mark, a newer version, a group containing itself
    let mut bad = j.clone();
    bad.as_object_mut().unwrap().remove("format");
    assert!(native::load_native(&bad).is_err());
    let mut newer = j.clone();
    newer["version"] = json!(99);
    let e = native::load_native(&newer).unwrap_err().to_string();
    assert!(e.contains("newer"), "{e}");
    let mut cyc = j.clone();
    let defs = cyc["defs"].as_array_mut().unwrap();
    let w = defs.iter_mut().find(|d| d["id"] == json!(fx.wall)).unwrap();
    w["insts"].as_array_mut().unwrap().push(json!({ "id": 999, "def": fx.wall, "m": IDENT.to_vec() }));
    assert!(native::load_native(&cyc).unwrap_err().to_string().contains("itself"));
}

#[test]
fn glb_export_and_reimport() {
    let fx = formats_model();
    let glb = gltf::export_glb(&fx.m);
    assert_eq!(&glb[0..4], b"glTF");
    assert_eq!(glb.len() % 4, 0, "4-byte padding");
    let jl = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let j: Value = serde_json::from_slice(&glb[20..20 + jl]).unwrap();
    assert_eq!(j["meshes"].as_array().unwrap().len(), 4, "one mesh per definition with faces");
    let col_mesh = j["meshes"].as_array().unwrap().iter().position(|x| x["name"] == "Column").unwrap();
    assert_eq!(j["nodes"].as_array().unwrap().iter().filter(|n| n["mesh"] == json!(col_mesh)).count(), 3, "the column mesh is shared");
    assert_eq!(j["asset"]["generator"], "ModelCraft");
    let mut m2 = Model::new();
    let r = gltf::import_gltf(&mut m2, &glb, "model.glb").unwrap();
    let n_tris = cc_m3d::mesh::world_triangles(&fx.m, "root", &IDENT).len();
    assert_eq!(r.report["triangles"], json!(n_tris as f64));
    assert!(close(def_area(m2.def(&r.def).unwrap()), world_area(&fx.m), 1e-6));
    // a face colour goes out as a material and comes back on its 2 triangles
    let mut mc = Model::new();
    {
        let root = mc.root_mut().unwrap();
        add_polygon(root, &rect_pts([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 1.0, 1.0), None).unwrap();
        let f = faces_of(root)[0];
        push_pull(root, f, 1.0);
        let top = root.faces.iter().find(|(_, f)| face_normal(root, f)[2] > 0.99).map(|(id, _)| id).unwrap();
        root.faces.get_mut(top).unwrap().color = Some("#c0392b".into());
    }
    let g2 = gltf::export_glb(&mc);
    let mut back = Model::new();
    let r2 = gltf::import_gltf(&mut back, &g2, "c").unwrap();
    let coloured: Vec<_> = back.def(&r2.def).unwrap().faces.values().filter_map(|f| f.color.clone()).collect();
    assert_eq!(coloured, vec!["#c0392b".to_string(), "#c0392b".to_string()]);
}

#[test]
fn gltf_refusals_and_variants() {
    let refuse = |bytes: &[u8], want: &str| {
        let e = gltf::import_gltf(&mut Model::new(), bytes, "x").unwrap_err().to_string();
        assert!(e.contains(want), "{e} should say {want}");
    };
    let glb = gltf::export_glb(&formats_model().m);
    let mut cut = glb[..glb.len() - 100].to_vec();
    let n = cut.len() as u32;
    cut[8..12].copy_from_slice(&n.to_le_bytes());
    refuse(&cut, "could not be read");
    refuse(json!({ "asset": { "version": "2.0" }, "buffers": [{ "uri": "model.bin", "byteLength": 10 }] }).to_string().as_bytes(), ".glb");
    refuse(json!({ "asset": { "version": "1.0" } }).to_string().as_bytes(), "2.0");
    let past = json!({ "asset": { "version": "2.0" }, "buffers": [{ "uri": "data:application/octet-stream;base64,AAAAAAAAAAAAAAAA", "byteLength": 12 }],
        "bufferViews": [{ "buffer": 0, "byteLength": 12 }], "accessors": [{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3" }],
        "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 } }] }], "nodes": [{ "mesh": 0 }], "scenes": [{ "nodes": [0] }] });
    refuse(past.to_string().as_bytes(), "past");
    refuse(
        json!({ "asset": { "version": "2.0" }, "scene": 0, "scenes": [{ "nodes": [0] }], "nodes": [{ "translation": ["x", 0, 0] }] })
            .to_string()
            .as_bytes(),
        "node transform is not a number",
    );
    // a .gltf with a data: URI, a TRS node, a triangle strip and a line primitive
    let pos: Vec<u8> = [0f32, 0., 0., 1., 0., 0., 0., 1., 0., 1., 1., 0.].iter().flat_map(|f| f.to_le_bytes()).collect();
    let b64 = cc_m3d::paint::to_base64(&pos);
    let gl = json!({ "asset": { "version": "2.0" }, "buffers": [{ "uri": format!("data:application/octet-stream;base64,{b64}"), "byteLength": 48 }],
        "bufferViews": [{ "buffer": 0, "byteLength": 48 }], "accessors": [{ "bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3" }],
        "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 }, "mode": 5 }, { "attributes": { "POSITION": 0 }, "mode": 1 }] }],
        "nodes": [{ "mesh": 0, "translation": [5, 0, 0], "scale": [2, 2, 2] }], "scenes": [{ "nodes": [0] }], "scene": 0 });
    let mut m = Model::new();
    let r = gltf::import_gltf(&mut m, gl.to_string().as_bytes(), "strip").unwrap();
    assert_eq!(r.report["triangles"], json!(2.0));
    assert_eq!(r.report["skipped"]["lines"], json!(1.0));
    let d = m.def(&r.def).unwrap();
    assert!(close(def_area(d), 4.0, 1e-9));
    assert!(d.verts.values().all(|p| p[0] >= 5.0 - 1e-9 && p[0] <= 7.0 + 1e-9 && close(p[1], 0.0, 1e-12)), "a 2 × 2 square standing in XZ");
}

#[test]
fn obj_export_and_import() {
    let fx = formats_model();
    let text = obj::export_obj(&fx.m, None);
    assert!(text.lines().any(|l| l.starts_with("l ") && l.split(' ').count() == 3), "the loose edge as an l line");
    assert_eq!(text.lines().filter(|l| l.starts_with("o ")).count(), 6, "one object per placed definition");
    let mut m2 = Model::new();
    let r = obj::import_obj(&mut m2, &text, "model.obj").unwrap();
    assert!(close(def_area(m2.def(&r.def).unwrap()), world_area(&fx.m), 1e-5));
    assert_eq!(r.report["skipped"]["lines"], json!(1.0), "the l line is counted as skipped");
    let r = obj::import_obj(
        &mut Model::new(),
        "# t\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nvt 0 0\nvn 0 0 1\nf -4/1/1 -3/1/1 -2/1/1 -1/1/1\nf 1 2 9\nf 1 1 1\n",
        "q",
    )
    .unwrap();
    assert_eq!(r.report["faces"], json!(1.0));
    assert_eq!(r.report["skipped"]["bad face"], json!(1.0));
    assert!(obj::import_obj(&mut Model::new(), "v 1 2 x\n", "bad").is_err(), "a vertex that is not a number is refused");
}

#[test]
fn stl_export() {
    let fx = formats_model();
    let s = stl::export_stl(&fx.m);
    assert_eq!(&s[..27], b"ModelCraft STL, millimeters");
    let n = u32::from_le_bytes(s[80..84].try_into().unwrap()) as usize;
    assert_eq!(n, cc_m3d::mesh::world_triangles(&fx.m, "root", &IDENT).len());
    assert_eq!(s.len(), 84 + 50 * n);
}

fn site_dxf() -> String {
    [
        "0",
        "SECTION",
        "2",
        "HEADER",
        "9",
        "$INSUNITS",
        "70",
        "4",
        "0",
        "ENDSEC",
        "0",
        "SECTION",
        "2",
        "BLOCKS",
        "0",
        "BLOCK",
        "8",
        "0",
        "2",
        "TREE",
        "70",
        "0",
        "10",
        "0",
        "20",
        "0",
        "30",
        "0",
        "0",
        "CIRCLE",
        "8",
        "0",
        "10",
        "0",
        "20",
        "0",
        "30",
        "0",
        "40",
        "500",
        "0",
        "ENDBLK",
        "8",
        "0",
        "0",
        "ENDSEC",
        "0",
        "SECTION",
        "2",
        "ENTITIES",
        "0",
        "LINE",
        "8",
        "Site",
        "10",
        "0",
        "20",
        "0",
        "30",
        "7",
        "11",
        "10000",
        "21",
        "0",
        "31",
        "7",
        "0",
        "LWPOLYLINE",
        "8",
        "Site",
        "90",
        "4",
        "70",
        "1",
        "10",
        "0",
        "20",
        "-5000",
        "10",
        "2000",
        "20",
        "-5000",
        "10",
        "2000",
        "20",
        "-3000",
        "10",
        "0",
        "20",
        "-3000",
        "42",
        "1",
        "0",
        "CIRCLE",
        "8",
        "Site",
        "10",
        "20000",
        "20",
        "0",
        "30",
        "0",
        "40",
        "1000",
        "0",
        "ARC",
        "8",
        "Site",
        "10",
        "30000",
        "20",
        "0",
        "30",
        "0",
        "40",
        "1000",
        "50",
        "0",
        "51",
        "90",
        "0",
        "INSERT",
        "8",
        "Trees",
        "2",
        "TREE",
        "10",
        "40000",
        "20",
        "0",
        "30",
        "0",
        "41",
        "2",
        "42",
        "2",
        "50",
        "30",
        "0",
        "TEXT",
        "8",
        "Site",
        "10",
        "0",
        "20",
        "-20000",
        "40",
        "1000",
        "1",
        "Lot 4",
        "0",
        "LINE",
        "8",
        "Sheet",
        "67",
        "1",
        "10",
        "0",
        "20",
        "0",
        "11",
        "5",
        "21",
        "5",
        "0",
        "LINE",
        "8",
        "Site",
        "60",
        "1",
        "10",
        "0",
        "20",
        "90000",
        "11",
        "5",
        "21",
        "90000",
        "0",
        "ENDSEC",
        "0",
        "EOF",
        "",
    ]
    .join("\n")
}

#[test]
fn dxf_site_plan() {
    let mut m = Model::new();
    let r = site::import_dxf(&mut m, &site_dxf(), "site.dxf", None).unwrap();
    assert_eq!(r.units.as_deref(), Some("mm"));
    let rep = &r.report;
    for (k, v) in
        [("lines", 1.0), ("polylines", 1.0), ("circles", 2.0), ("arcs", 1.0), ("inserts", 1.0), ("texts", 1.0), ("paperSpace", 1.0)]
    {
        assert_eq!(rep[k], json!(v), "{k}");
    }
    let d = m.def(&r.def).unwrap();
    assert!(d.faces.is_empty());
    assert!(d.verts.values().all(|p| p[2] == 0.0), "every vertex on the ground");
    assert!(!d.verts.values().any(|p| p[1] > 80.0), "the invisible line is left out");
    let lot: Vec<&P3> = d.verts.values().filter(|p| p[1] < -18.5).collect();
    assert!(lot.len() > 20 && close(lot.iter().map(|p| p[1]).fold(f64::MIN, f64::max), -19.0, 1e-9), "Lot 4 drawn 1 m tall");
    let tree: Vec<&P3> = d.verts.values().filter(|p| p[0] > 38.0).collect();
    assert_eq!(tree.len(), 48, "the inserted block: a 48-segment circle");
    assert!(tree.iter().all(|p| close(cc_m3d::vec::dist(**p, [40.0, 0.0, 0.0]), 1.0, 1e-9)));
    // no $INSUNITS: units null, the caller's unit used
    let no_units = site_dxf().replace("$INSUNITS\n70\n4", "$ACADVER\n1\nAC1009");
    let r2 = site::import_dxf(&mut Model::new(), &no_units, "x", Some("ft")).unwrap();
    assert!(r2.units.is_none());
    let e = site::import_dxf(&mut Model::new(), "AutoCAD Binary DXF\r\n\u{1a}\u{0}", "b", None).unwrap_err().to_string();
    assert!(e.to_lowercase().contains("binary"), "{e}");
}

#[test]
fn dwg_site_plan_model() {
    let sq = json!({ "header": { "INSUNITS": 6 }, "blocks": [{ "name": "*Model_Space", "entities": [
        { "type": "LWPOLYLINE", "flags": 512, "closed": true, "vertices": [{ "x": 0, "y": 0, "bulge": 0 }, { "x": 1, "y": 0, "bulge": 0 }, { "x": 1, "y": 1, "bulge": 0 }, { "x": 0, "y": 1, "bulge": 0 }] },
        { "type": "LWPOLYLINE", "flags": 1, "closed": false, "vertices": [{ "x": 5, "y": 0, "bulge": 1 }, { "x": 7, "y": 0, "bulge": 0 }] },
        { "type": "LINE", "invisible": true, "start": { "x": 0, "y": 5, "z": 0 }, "end": { "x": 9, "y": 5, "z": 0 } },
        { "type": "INSERT", "block": "nope", "insert": { "x": 0, "y": 0, "z": 0 } } ] }] });
    let mut m = Model::new();
    let r = site::import_dwg(&mut m, &sq, "sq", None).unwrap();
    assert_eq!(r.units.as_deref(), Some("m"));
    let d = m.def(&r.def).unwrap();
    let half = d.edges.values().filter(|e| d.v(e.a)[0] >= 5.0).count();
    assert_eq!(half, 24, "an open bulge polyline is a 24-segment half circle");
    assert_eq!(r.report["skipped"]["INSERT (missing block)"], json!(1.0));
    assert!(site::import_dwg(&mut Model::new(), &json!({ "blocks": 3 }), "x", None).is_err());
}

fn cad_model() -> Model {
    let mut mx = Model::new();
    mx.units = "mm".into();
    {
        let root = mx.root_mut().unwrap();
        add_polygon(root, &rect_pts([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 2.0, 1.0), None).unwrap();
        let f = faces_of(root)[0];
        push_pull(root, f, 0.5);
        let top = root.faces.iter().find(|(_, f)| close(face_normal(root, f)[2], 1.0, 1e-9)).map(|(id, _)| id).unwrap();
        root.faces.get_mut(top).unwrap().color = Some("#c0392b".into());
        cc_m3d::edges::add_edge(root, [3.0, 0.0, 0.0], [3.0, 2.0, 0.0], Default::default());
    }
    let cd = mx.new_def("component", Some("Post"));
    {
        let d = mx.def_mut(&cd).unwrap();
        add_polygon(d, &rect_pts([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0.2, 0.2), None).unwrap();
        let f = faces_of(d)[0];
        push_pull(d, f, 1.5);
    }
    let mirror: M4 = [-1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    for pm in [
        translation([5.0, 0.0, 0.0]),
        mmul(&translation([7.0, 0.0, 0.0]), &rotation([1.0, 0.0, 0.0], std::f64::consts::PI / 2.0, [0.0; 3])),
        mmul(&translation([9.0, 0.0, 0.0]), &mirror),
    ] {
        place(mx.root_mut().unwrap(), &cd, &pm);
    }
    let gd = mx.new_def("group", Some("Slab"));
    {
        let d = mx.def_mut(&gd).unwrap();
        add_polygon(d, &rect_pts([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 1.0, 1.0), None).unwrap();
        let f = faces_of(d)[0];
        push_pull(d, f, 0.1);
    }
    let stretch: M4 = [2.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    let skew = mmul(&translation([12.0, 0.0, 0.0]), &mmul(&stretch, &rotation([0.0, 0.0, 1.0], std::f64::consts::PI / 6.0, [0.0; 3])));
    place(mx.root_mut().unwrap(), &gd, &skew);
    mx
}

#[test]
fn dxf_and_dwg_export() {
    let mx = cad_model();
    let (text, rep) = cad::export_dxf(&mx).unwrap();
    assert_eq!((rep.blocks, rep.inserts, rep.flattened, rep.lines, rep.unit.as_str()), (1.0, 3.0, 1.0, 1.0, "millimeters"));
    assert!(text.starts_with("0\r\nSECTION\r\n2\r\nHEADER\r\n9\r\n$ACADVER\r\n1\r\nAC1009"));
    assert!(text.ends_with("\r\n0\r\nEOF\r\n"));
    let red = cad::nearest_aci(Some("#c0392b"));
    assert_ne!(red, 7.0);
    assert!(text.contains(&format!("3DFACE\r\n8\r\n0\r\n62\r\n{}\r\n", cc_m3d::js::num_str(red))));
    let (dwg, rep2) = cad::to_dwg_model(&mx).unwrap();
    assert_eq!(dwg["header"]["INSUNITS"], json!(4.0));
    let inserts = dwg["blocks"][0]["entities"].as_array().unwrap().iter().filter(|e| e["type"] == "INSERT").count();
    assert_eq!(inserts, 3);
    let tops = dwg["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|b| b["entities"].as_array().unwrap().iter())
        .filter(|e| e["type"] == "3DFACE" && e["color"]["rgb"] == json!(0xc0392b as f64))
        .count();
    assert_eq!(tops, 2, "the red top keeps its true colour");
    assert_eq!(rep2.triangles, rep.triangles);
    // a feet-and-inches model is written in inches
    let mut mi = cad_model();
    mi.units = "ft-in".into();
    assert_eq!(cad::export_dxf(&mi).unwrap().1.unit, "inches");
}

#[test]
fn paint_reaches_every_export() {
    let mut mp = Model::new();
    mp.units = "m".into();
    {
        let root = mp.root_mut().unwrap();
        add_polygon(root, &rect_pts([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 2.0, 1.0), None).unwrap();
        let f = faces_of(root)[0];
        push_pull(root, f, 0.5);
        let top = root.faces.iter().find(|(_, f)| close(face_normal(root, f)[2], 1.0, 1e-9)).map(|(id, _)| id).unwrap();
        cc_m3d::paint::paint_faces(root, &[top], Some("#C0392B")).unwrap();
    }
    let cd = mp.new_def("component", Some("Post"));
    {
        let d = mp.def_mut(&cd).unwrap();
        add_polygon(d, &rect_pts([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0.2, 0.2), None).unwrap();
        let f = faces_of(d)[0];
        push_pull(d, f, 1.0);
    }
    place(mp.root_mut().unwrap(), &cd, &translation([4.0, 0.0, 0.0]));
    let ib = place(mp.root_mut().unwrap(), &cd, &translation([5.0, 0.0, 0.0]));
    let ic = place(mp.root_mut().unwrap(), &cd, &translation([6.0, 0.0, 0.0]));
    cc_m3d::paint::paint_instance(mp.root_mut().unwrap(), ib, Some("#2b5fbd")).unwrap();
    cc_m3d::paint::paint_instance(mp.root_mut().unwrap(), ic, Some("#2e9d57")).unwrap();
    assert!(mp.root().unwrap().faces.values().any(|f| f.color.as_deref() == Some("#c0392b")), "stored in lower case");
    let o = obj::export_obj(&mp, Some("paint.mtl"));
    let mtl = obj::export_mtl(&mp);
    assert!(o.lines().any(|l| l == "mtllib paint.mtl"));
    assert_eq!(mtl.lines().filter(|l| l.starts_with("newmtl ")).count(), 4, "the default and 3 colours");
    let (text, _) = cad::export_dxf(&mp).unwrap();
    assert_eq!(text.matches("\r\nBLOCK\r\n").count(), 3, "a block per colour the post is shown in");
    let glb = gltf::export_glb(&mp);
    let jl = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let j: Value = serde_json::from_slice(&glb[20..20 + jl]).unwrap();
    assert_eq!(j["meshes"].as_array().unwrap().len(), 4, "box, post, blue post, green post");
}

#[test]
fn collada_export_and_reimport() {
    let fx = formats_model();
    let mut mc = native::load_native(&serde_json::from_str(&native::export_native(&mut { fx.m })).unwrap()).unwrap();
    {
        let root = mc.root_mut().unwrap();
        let top = root.faces.iter().find(|(_, f)| dot(face_normal(root, f), [0.0, 0.0, 1.0]) > 0.999).map(|(id, _)| id).unwrap();
        root.faces.get_mut(top).unwrap().color = Some("#cc3300".into());
    }
    for f in mc.def_mut(&fx.wall).unwrap().faces.values_mut() {
        f.color = Some("#336699".into());
    }
    let dae = collada::export_dae(&mc, "Test model", DATE);
    assert!(dae.starts_with("<?xml") && dae.contains("<up_axis>Z_UP</up_axis>") && dae.contains("<unit name=\"meter\" meter=\"1\"/>"));
    assert_eq!(dae.matches("<geometry ").count(), 4);
    assert_eq!(dae.matches("<instance_node ").count(), 3);
    assert_eq!(dae.matches("<node id=\"comp-").count(), 1);
    assert_eq!(dae.matches("<polylist ").count(), 5);
    assert!(dae.contains("<lines count=\"1\">"));
    assert!(dae.contains("<created>2026-10-10T12:00:00Z</created>"));
    let mut m3 = Model::new();
    let r = collada::import_dae(&mut m3, &dae, "model").unwrap();
    let comps: Vec<&Def> = m3.defs.values().filter(|d| d.kind == "component").collect();
    assert_eq!(comps.len(), 1);
    assert_eq!(comps[0].name, "Column");
    let w3 = m3.defs.values().find(|d| d.name == "Wall with opening").unwrap();
    let si = cc_m3d::solids::solid_info(w3);
    assert!(w3.kind == "group" && si.closed && close(si.volume.unwrap(), 3.6 - 0.45, 1e-9));
    assert!(w3.faces.values().all(|f| f.color.as_deref() == Some("#336699")));
    assert_eq!(m3.def(&r.def).unwrap().name, "model");
}

#[test]
fn collada_hand_written() {
    let x = |up: &str, unit: &str, n: usize, pts: &str, prims: &str, extra: &str| -> String {
        format!(
            "<?xml version=\"1.0\"?>\n<!DOCTYPE COLLADA [<!ENTITY boom \"BOOM\">]>\n<COLLADA xmlns=\"http://www.collada.org/2005/11/COLLADASchema\" version=\"1.4.1\"><asset><unit meter=\"{unit}\"/><up_axis>{up}</up_axis></asset>\
             <library_geometries><geometry id=\"g\" name=\"Shapes &amp; holes\"><mesh><source id=\"p\"><float_array id=\"pa\" count=\"0\">{pts}</float_array><technique_common><accessor source=\"#pa\" count=\"{n}\" stride=\"3\"/></technique_common></source>\
             <vertices id=\"v\"><input semantic=\"POSITION\" source=\"#p\"/></vertices>{prims}</mesh></geometry></library_geometries>\
             <library_visual_scenes><visual_scene id=\"s\"><node name=\"n &boom;\"><lookat>0 0 1 0 0 0 0 1 0</lookat><instance_geometry url=\"#g\"/>{extra}</node></visual_scene></library_visual_scenes><scene><instance_visual_scene url=\"#s\"/></scene></COLLADA>"
        )
    };
    let pts = "0 0 0 100 0 0 100 100 0 0 100 0 25 25 0 75 25 0 75 75 0 25 75 0 200 0 0 300 0 0 300 100 0 200 100 0 400 0 0 500 0 0 400 100 0 500 100 0 0 200 0 100 200 0";
    let prims = "<polygons count=\"1\"><input semantic=\"VERTEX\" source=\"#v\" offset=\"0\"/><ph><p>0 1 2 3</p><h>4 7 6 5</h></ph></polygons>\
                 <trifans count=\"1\"><input semantic=\"VERTEX\" source=\"#v\" offset=\"0\"/><p>8 9 10 11</p></trifans>\
                 <tristrips count=\"1\"><input semantic=\"VERTEX\" source=\"#v\" offset=\"0\"/><p>12 13 14 15</p></tristrips>\
                 <lines count=\"1\"><input semantic=\"VERTEX\" source=\"#v\" offset=\"0\"/><p>16 17</p></lines>";
    let mut m = Model::new();
    let r = collada::import_dae(
        &mut m,
        &x(
            "Z_UP",
            "0.01",
            18,
            pts,
            prims,
            "<node name=\"ext\"><instance_node url=\"other.dae#thing\"/></node><instance_light url=\"#nolight\"/>",
        ),
        "hand",
    )
    .unwrap();
    let d = m.def(&r.def).unwrap();
    let holed = d.faces.values().find(|f| f.loops.len() == 2).unwrap();
    assert!(close(face_area(d, holed), 0.75, 1e-9));
    assert!(close(def_area(d), 2.75, 1e-9));
    for k in ["lookat transforms", "references to other files", "cameras and lights"] {
        assert_eq!(r.report["skipped"][k], json!(1.0), "{k}");
    }
    let tri = "<triangles count=\"1\"><input semantic=\"VERTEX\" source=\"#v\" offset=\"0\"/><p>0 1 2</p></triangles>";
    let mut m6 = Model::new();
    let r6 = collada::import_dae(&mut m6, &x("X_UP", "1", 3, "0 0 0 1 0 0 0 1 0", tri, ""), "x").unwrap();
    let mut p6: Vec<String> = m6
        .def(&r6.def)
        .unwrap()
        .verts
        .values()
        .map(|p| p.iter().map(|v| cc_m3d::js::num_str((v * 1e9).round() / 1e9 + 0.0)).collect::<Vec<_>>().join(","))
        .collect();
    p6.sort();
    assert_eq!(p6, vec!["-1,0,0", "0,0,0", "0,0,1"]);
    let bad = |text: &str, want: &str| {
        let e = collada::import_dae(&mut Model::new(), text, "bad").unwrap_err().to_string();
        assert!(e.contains(want), "{e} should say {want}");
    };
    bad("solid cube\nfacet normal 0 0 1", "not well-formed");
    bad("<COLLADA><asset></COLLADA>", "does not match");
    bad("<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\"/>", "not a Collada");
    bad(&x("Z_UP", "1", 3, "0 0 0 1 0 0 0 1 0", &tri.replace("0 1 2", "0 1 9"), ""), "index");
    bad(&x("Z_UP", "1", 3, "0 0 0 1 0", tri, ""), "shorter");
    bad(&x("Z_UP", "1", 3, "0 0 0 1 0 x 0 1 0", tri, ""), "not a number");
    bad(&x("Z_UP", "1", 0, "", "", ""), "no geometry");
}

#[test]
fn ifc_export() {
    let fx = formats_model();
    let text = ifc::export_ifc(&fx.m, "Pavilion — test", DATE);
    assert!(text.starts_with("ISO-10303-21;") && text.contains("FILE_SCHEMA(('IFC4'));"));
    assert_eq!(text, ifc::export_ifc(&fx.m, "Pavilion — test", DATE), "deterministic GlobalIds");
    assert!(text.contains("'Pavilion \\X2\\2014\\X0\\ test'"));
    assert_eq!(text.matches("IFCBUILDINGELEMENTPROXY(").count(), 6, "3 columns, wall, pad, loose geometry");
    assert_eq!(text.matches("IFCFACETEDBREP(").count(), 5);
    assert_eq!(text.matches("IFCSHELLBASEDSURFACEMODEL(").count(), 1);
    let ids: Vec<&str> = text
        .split("('")
        .skip(1)
        .filter_map(|s| s.get(..22))
        .filter(|s| s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$'))
        .collect();
    let mut uniq = ids.clone();
    uniq.sort();
    uniq.dedup();
    assert!(ids.len() > 8 && uniq.len() == ids.len(), "GlobalIds all distinct");
    assert_eq!(ifc::step_string("it's \\ ok"), "'it''s \\\\ ok'");
}

/// Fuzz find (m3d_import oom-5a5c1802…): nested inserts of 3DFACEs made millions of filled outlines before the
/// 200 000 check ran. The drawing now stops as soon as the count is passed, with the same refusal.
#[test]
fn dxf_site_plan_nested_faces_refused_early() {
    let mut d = String::from("0\nSECTION\n2\nBLOCKS\n");
    d.push_str("0\nBLOCK\n8\n0\n2\nB0\n70\n0\n10\n0\n20\n0\n30\n0\n");
    for i in 0..10 {
        d.push_str(&format!(
            "0\n3DFACE\n8\n0\n10\n{i}\n20\n0\n30\n0\n11\n{}\n21\n0\n31\n0\n12\n{i}\n22\n1\n32\n0\n13\n{i}\n23\n1\n33\n0\n",
            i + 1
        ));
    }
    d.push_str("0\nENDBLK\n");
    for b in 1..=6 {
        d.push_str(&format!("0\nBLOCK\n8\n0\n2\nB{b}\n70\n0\n10\n0\n20\n0\n30\n0\n"));
        for i in 0..10 {
            d.push_str(&format!("0\nINSERT\n8\n0\n2\nB{}\n10\n{i}\n20\n0\n30\n0\n", b - 1));
        }
        d.push_str("0\nENDBLK\n");
    }
    d.push_str("0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nINSERT\n8\n0\n2\nB6\n10\n0\n20\n0\n30\n0\n0\nENDSEC\n0\nEOF\n");
    let e = site::import_dxf(&mut Model::new(), &d, "nest.dxf", None).unwrap_err().to_string();
    assert!(e.contains("200"), "{e}");
}
