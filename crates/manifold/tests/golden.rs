//! Golden parity: synthetic boolean scripts (walls with openings, lists of solids, pairs, unwelded soups, grids of
//! touching boxes, operation chains, extra properties, survey-scale coordinates, malformed meshes) and the outputs
//! manifold-3d 3.5.4's wasm gave for them (tests/golden.json, written by `forks/tools/mfdiff.mjs --golden`). Every
//! output must match exactly: statuses, counts, order, run IDs, floats bit for bit.
//!
//! One test function runs the cases in order: mesh IDs come from a process-wide counter, as in the C++, so this
//! binary must not run anything else that makes meshes.

use std::collections::HashMap;

use cc_manifold::{Manifold, Mesh, OpType};
use serde_json::{Value, json};

fn u32s(v: &Value) -> Vec<u32> {
    v.as_array().unwrap().iter().map(|x| u32::try_from(x.as_u64().unwrap()).unwrap()).collect()
}

fn bits(v: &[f32]) -> Value {
    Value::Array(v.iter().map(|f| Value::from(f.to_bits())).collect())
}

fn mesh_json(m: &Mesh) -> Value {
    json!({
        "numProp": m.num_prop,
        "vertProperties": bits(&m.vert_properties),
        "triVerts": m.tri_verts,
        "mergeFromVert": m.merge_from_vert,
        "mergeToVert": m.merge_to_vert,
        "runIndex": m.run_index,
        "runOriginalID": m.run_original_id,
        "runTransform": bits(&m.run_transform),
        "runFlags": m.run_flags,
        "faceID": m.face_id,
        "halfedgeTangent": bits(&m.halfedge_tangent),
        "tolerance": m.tolerance.to_bits(),
    })
}

/// The step semantics of `cadcraft-cli manifold` / mfdiff.mjs (a step naming a missing handle is skipped).
fn run(case: &Value) -> Vec<Value> {
    let meshes: Vec<Mesh> = case["meshes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            let vp = u32s(&m["vertBits"]).into_iter().map(f32::from_bits).collect();
            Mesh::new(u32::try_from(m["numProp"].as_u64().unwrap()).unwrap(), vp, u32s(&m["triVerts"]))
        })
        .collect();
    let mut vars: HashMap<String, Manifold> = HashMap::new();
    let mut out = Vec::new();
    for st in case["steps"].as_array().unwrap() {
        let st = st.as_array().unwrap();
        let op = st[0].as_str().unwrap();
        let id = st[1].as_str().unwrap().to_string();
        match op {
            "mesh" => {
                let mut m = meshes[usize::try_from(st[2].as_u64().unwrap()).unwrap()].clone();
                if st[3].as_bool().unwrap_or(false) {
                    m.merge();
                }
                match Manifold::from_mesh(&m) {
                    Ok(x) => {
                        vars.insert(id, x);
                        out.push(json!({ "ctor": "NoError" }));
                    }
                    Err(s) => out.push(json!({ "ctor": s.as_str() })),
                }
            }
            "subtract" | "add" | "intersect" => {
                let (Some(a), Some(b)) = (vars.get(st[2].as_str().unwrap()), vars.get(st[3].as_str().unwrap())) else { continue };
                let r = match op {
                    "subtract" => a.subtract(b),
                    "add" => a.add(b),
                    _ => a.intersect(b),
                };
                vars.insert(id, r);
            }
            "union" | "intersection" | "difference" => {
                let list: Option<Vec<Manifold>> =
                    st[2].as_array().unwrap().iter().map(|k| vars.get(k.as_str().unwrap()).cloned()).collect();
                let Some(list) = list else { continue };
                let opt = match op {
                    "union" => OpType::Add,
                    "intersection" => OpType::Intersect,
                    _ => OpType::Subtract,
                };
                let r = Manifold::batch_boolean(&list, opt);
                drop(list);
                vars.insert(id, r);
            }
            "delete" => {
                vars.remove(&id);
            }
            _ => {
                let Some(m) = vars.get(&id) else {
                    out.push(json!({ "missing": true }));
                    continue;
                };
                out.push(match op {
                    "get" => json!({ "mesh": mesh_json(&m.get_mesh()) }),
                    "status" => json!({ "status": m.status().as_str() }),
                    _ => json!({ "isEmpty": m.is_empty() }),
                });
            }
        }
    }
    out
}

#[test]
fn golden_cases_match_manifold_3d_wasm() {
    let doc: Value = serde_json::from_str(include_str!("golden.json")).unwrap();
    let cases = doc["cases"].as_array().unwrap();
    let expected = doc["expected"].as_array().unwrap();
    assert_eq!(cases.len(), expected.len());
    for (c, e) in cases.iter().zip(expected) {
        let got = Value::Array(run(c));
        assert!(got == *e, "case {} differs from manifold-3d", c["name"]);
    }
}
