//! `cadcraft-cli manifold-hull <cases.json> [--bench N]`: convex hulls through cc-manifold (`Manifold::hull_points`),
//! for the parity harness `forks/tools/hulldiff.mjs` (which runs `Manifold.hull(points)` in manifold-3d's wasm).
//!
//! Input: `{"cases": [{"name", "hulls": [[x, y, z bits as 16-digit hex strings, ...], ...]}]}`: each hull is a flat
//! list of coordinates given as their IEEE-754 bit patterns (exact, -0 included). All hulls of all cases run in one
//! process, in order, so the mesh IDs advance as in one wasm instance.
//! Output: `{"cases": [{"name", "out": [{"mesh": …} | {"status": …}, ...]}]}` (floats as u32 bit patterns, like
//! `cadcraft-cli manifold`); `--bench N`: also the best of N runs of the case's hulls in ms (`"ms"`).

use std::process::ExitCode;
use std::time::Instant;

use cc_manifold::{Manifold, Mesh};
use serde_json::{Map, Value, json};

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

fn points(v: &Value) -> Result<Vec<[f64; 3]>, String> {
    let a = v.as_array().ok_or("a hull is an array")?;
    let mut flat = Vec::with_capacity(a.len());
    for x in a {
        let s = x.as_str().ok_or("coordinate bits as a hex string")?;
        let b = u64::from_str_radix(s, 16).map_err(|e| e.to_string())?;
        flat.push(f64::from_bits(b));
    }
    let (chunks, rest) = flat.as_chunks::<3>();
    if !rest.is_empty() {
        return Err("coordinates not a multiple of 3".into());
    }
    Ok(chunks.to_vec())
}

pub fn run(rest: &[&str]) -> ExitCode {
    let Some(path) = rest.first() else {
        eprintln!("usage: cadcraft-cli manifold-hull <cases.json> [--bench N]");
        return ExitCode::from(2);
    };
    let runs: usize = rest.iter().position(|a| *a == "--bench").and_then(|i| rest.get(i + 1)).and_then(|n| n.parse().ok()).unwrap_or(0);
    let doc: Value =
        match std::fs::read_to_string(path).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string())) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::from(1);
            }
        };
    let Some(cases) = doc.get("cases").and_then(Value::as_array) else {
        eprintln!("no cases");
        return ExitCode::from(1);
    };
    let mut results = Vec::with_capacity(cases.len());
    for case in cases {
        let mut obj = Map::new();
        obj.insert("name".into(), case.get("name").cloned().unwrap_or(Value::Null));
        let hulls: Result<Vec<Vec<[f64; 3]>>, String> =
            case.get("hulls").and_then(Value::as_array).ok_or_else(|| "hulls".to_string()).and_then(|a| a.iter().map(points).collect());
        match hulls {
            Err(e) => {
                obj.insert("error".into(), Value::from(e));
            }
            Ok(hulls) => {
                let mut out = Vec::new();
                for pts in &hulls {
                    let m = Manifold::hull_points(pts);
                    out.push(json!({ "status": m.status().as_str() }));
                    out.push(json!({ "mesh": mesh_json(&m.get_mesh()) }));
                }
                obj.insert("out".into(), Value::Array(out));
                if runs > 0 {
                    let mut best = f64::INFINITY;
                    for _ in 0..runs {
                        let t = Instant::now();
                        for pts in &hulls {
                            let m = Manifold::hull_points(pts);
                            std::hint::black_box(m.num_tri());
                        }
                        best = best.min(t.elapsed().as_secs_f64() * 1000.0);
                    }
                    obj.insert("ms".into(), Value::from(best));
                }
            }
        }
        results.push(Value::Object(obj));
    }
    println!("{}", json!({ "cases": results }));
    ExitCode::SUCCESS
}
