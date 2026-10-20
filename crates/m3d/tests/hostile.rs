//! Hostile files: what the kernel must survive without hanging, overflowing its stack or running out of memory.

use cc_m3d::Model;
use serde_json::{Value, json};

fn file(defs: Vec<Value>) -> Value {
    json!({ "format": "modelcraft", "version": 1, "defs": defs })
}
const IDENT: [f64; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

/// 40 components each placing two of the next: 2^32 placements below the root. The JS walks them all (the page
/// hangs); the port stops after its walk budget, so it finishes.
#[test]
fn exponential_nesting_finishes() {
    let mut defs = vec![json!({ "id": "root", "insts": [{ "id": 1, "def": "d1", "m": IDENT }] })];
    for i in 1..=40 {
        let next = format!("d{}", i + 1);
        let insts = if i < 40 { json!([{ "id": 1, "def": next, "m": IDENT }, { "id": 2, "def": next, "m": IDENT }]) } else { json!([]) };
        defs.push(
            json!({ "id": format!("d{i}"), "kind": "component", "verts": [[1, 0, 0, 0], [2, 1, 0, 0], [3, 0, 1, 0]], "insts": insts }),
        );
    }
    let mut m = Model::new();
    m.load(&file(defs), false).unwrap();
    let t = std::time::Instant::now();
    let tris = cc_m3d::mesh::world_triangles(&m, "root", &cc_m3d::vec::IDENT);
    assert!(tris.is_empty());
    let _ = m.capture_vis();
    assert!(m.bbox("root", &cc_m3d::vec::IDENT).is_some());
    assert!(t.elapsed().as_secs() < 60, "the walk budget bounds the work");
}

/// 5,000 groups nested one in the next: the self-containment check runs without recursion.
#[test]
fn deep_nesting_loads() {
    let n = 5000;
    let mut defs = vec![json!({ "id": "root", "insts": [{ "id": 1, "def": "d1", "m": IDENT }] })];
    for i in 1..=n {
        let insts = if i < n { json!([{ "id": 1, "def": format!("d{}", i + 1), "m": IDENT }]) } else { json!([]) };
        defs.push(json!({ "id": format!("d{i}"), "insts": insts }));
    }
    let mut m = Model::new();
    m.load(&file(defs), false).unwrap();
    assert_eq!(m.defs.len(), n + 1);
    // the walk stops 32 levels down, as the JS does
    let mut count = 0;
    m.walk("root", &cc_m3d::vec::IDENT, &mut |_, _, _, _| {
        count += 1;
        true
    });
    assert_eq!(count, 33);
}

/// A group that contains itself (through two others) is refused with the JS's message.
#[test]
fn cycles_are_refused() {
    let defs = vec![
        json!({ "id": "root", "insts": [{ "id": 1, "def": "d1", "m": IDENT }] }),
        json!({ "id": "d1", "insts": [{ "id": 1, "def": "d2", "m": IDENT }] }),
        json!({ "id": "d2", "insts": [{ "id": 1, "def": "d3", "m": IDENT }] }),
        json!({ "id": "d3", "insts": [{ "id": 1, "def": "d1", "m": IDENT }] }),
    ];
    let mut m = Model::new();
    let e = m.load(&file(defs), false).unwrap_err();
    assert_eq!(e.to_string(), "This is not a ModelCraft file we can open: a group contains itself.");
}

/// Junk in every field: refused or left out, never a panic.
#[test]
fn junk_fields() {
    let cases = [
        json!(null),
        json!([]),
        json!({ "format": "modelcraft", "version": 1, "defs": [{ "id": "root", "verts": [[1, 1e300, 0, 0]] }] }),
        json!({ "format": "modelcraft", "version": 1, "defs": [{ "id": "root", "verts": [[1, 0, 0, 0], [2, 1, 0, 0]], "edges": [[3, 1, 1]] }] }),
        json!({ "format": "modelcraft", "version": 1, "defs": [{ "id": "root", "faces": [{ "id": 1, "loops": [[1, 2]] }] }] }),
        json!({ "format": "modelcraft", "version": 1, "nextId": 1e300, "defs": [{ "id": "root", "next": 1e300 }], "guides": [{ "id": -0.0, "kind": "point", "p": [0, 0, 0] }], "dims": [{ "id": 5, "off": [0, 0, 0], "kind": "rad", "c": { "p": [0, 0, 0], "n": [0, 0, 1], "r": 1, "path": [1, 2], "curve": "arc:1" } }], "scenes": [5, { "cam": { "eye": [0, 0, 1], "target": [0, 0, 0] }, "vis": [] }], "storeys": [{ "name": "a", "elevation": [3] }] }),
    ];
    for c in cases {
        let mut m = Model::new();
        let _ = m.load(&c, false);
        let _ = m.to_json_string(false);
        let _ = m.snapshot();
    }
    // ids at 1e300 stay JS numbers: every new id is the same number, as in the JS
    let mut m = Model::new();
    m.load(&json!({ "format": "modelcraft", "version": 1, "defs": [{ "id": "root", "next": 1e300 }] }), false).unwrap();
    let r = m.root_mut().unwrap();
    let a = cc_m3d::def::v_add(r, [0.0, 0.0, 0.0]);
    let b = cc_m3d::def::v_add(r, [1.0, 0.0, 0.0]);
    assert_eq!(a, b);
    assert_eq!(r.verts.len(), 1);
}

/// M6.18 (fuzz find, m3d_tools timeout in add_edge): a zero-length edge (two corners at one point) is split at a NaN
/// point by the next edge drawn, as the JS does. The halves, with their NaN end, are split again by every later edge,
/// doubling each time, as in the JS: twelve squares made millions of edges. Past MAX_NAN_EDGES they are left alone.
#[test]
fn nan_split_edges_do_not_multiply() {
    use cc_m3d::def::{Def, e_add0, v_add};
    use cc_m3d::edges::{EdgeOpts, add_edge};
    let mut d = Def::new("root", "group", "");
    let a = v_add(&mut d, [5.0, 5.0, 0.0]);
    let b = v_add(&mut d, [5.0, 5.0, 0.0]);
    e_add0(&mut d, a, b);
    let t = std::time::Instant::now();
    for k in 0..12 {
        let x = f64::from(k) * 0.1;
        let sq = [[x, 0.0, 0.0], [x + 1.0, 0.0, 0.0], [x + 1.0, 1.0, 0.0], [x, 1.0, 0.0]];
        for i in 0..4 {
            add_edge(&mut d, sq[i], sq[(i + 1) % 4], EdgeOpts::default());
        }
    }
    let nan = d.verts.values().filter(|p| p.iter().any(|x| !x.is_finite())).count();
    assert!(nan > 1 && nan < 4 * cc_m3d::edges::MAX_NAN_EDGES, "the NaN corners double up to the cap: {nan}");
    assert!(d.edges.len() < 8 * cc_m3d::edges::MAX_NAN_EDGES, "{} edges", d.edges.len());
    assert!(t.elapsed().as_secs() < 10);
}
