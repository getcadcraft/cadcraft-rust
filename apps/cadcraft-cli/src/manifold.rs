//! `cadcraft-cli manifold <cases.json> [--bench N]`: runs boolean scripts through cc-manifold and prints each output as
//! JSON, for the parity harness `forks/tools/mfdiff.mjs` (which runs the same scripts through manifold-3d's wasm).
//!
//! Input: `{"cases": [{"name", "meshes": [{"numProp", "vertProperties" | "vertBits", "triVerts"}], "steps": [...]}]}`
//! (`vertBits`: the floats as u32 bit patterns, exact for -0). Steps
//! mirror the JavaScript calls one for one (the CSG tree depends on which handles are still alive):
//!
//! ```text
//! ["mesh", "A", i, merge]      A = new Manifold(new Mesh(meshes[i])), after mesh.merge() when merge is true
//! ["subtract"|"add"|"intersect", "R", "A", "B"]
//! ["union"|"intersection"|"difference", "R", ["A", "B", ...]]
//! ["delete", "A"]              A.delete() (drops the handle)
//! ["get", "A"] | ["status", "A"] | ["isEmpty", "A"]   outputs
//! ```
//!
//! Output: `{"cases": [{"name", "out": [...]}]}`, one entry per `mesh` step (`{"ctor": status}`) and per output
//! step. Floats are printed as their IEEE bit patterns (u32) so the comparison is bit-exact (-0 included).
//! `--bench N`: also time each case (best of N runs after one warm-up, ms) as `"ms"`.

use std::collections::HashMap;
use std::process::ExitCode;
use std::time::Instant;

use cc_manifold::{Manifold, Mesh, OpType};
use serde_json::{Map, Value, json};

fn nums_u32(v: &Value) -> Result<Vec<u32>, String> {
    v.as_array()
        .ok_or("expected an array")?
        .iter()
        .map(|x| x.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| "expected a u32".to_string()))
        .collect()
}

fn nums_f32(v: &Value) -> Result<Vec<f32>, String> {
    #[allow(clippy::cast_possible_truncation)]
    v.as_array()
        .ok_or("expected an array")?
        .iter()
        .map(|x| x.as_f64().map(|n| n as f32).ok_or_else(|| "expected a number".to_string()))
        .collect()
}

fn parse_mesh(v: &Value) -> Result<Mesh, String> {
    let num_prop = v.get("numProp").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).ok_or("numProp")?;
    // vertBits: the floats as IEEE bit patterns (exact, -0 included); vertProperties: plain numbers
    let vp = match v.get("vertBits") {
        Some(b) => nums_u32(b)?.into_iter().map(f32::from_bits).collect(),
        None => nums_f32(v.get("vertProperties").ok_or("vertProperties")?)?,
    };
    let tv = nums_u32(v.get("triVerts").ok_or("triVerts")?)?;
    Ok(Mesh::new(num_prop, vp, tv))
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

/// An f64 as its IEEE bits, `[low u32, high u32]` (exact in JSON, -0 included).
fn f64_bits(x: f64) -> Value {
    let b = x.to_bits();
    json!([b & 0xffff_ffff, b >> 32])
}

fn nums_f64(v: &Value) -> Result<Vec<f64>, String> {
    v.as_array().ok_or("expected an array")?.iter().map(num_f64).collect()
}

/// A number, or `[lo, hi]` IEEE bits, or "NaN" / "Infinity" / "-Infinity".
fn num_f64(x: &Value) -> Result<f64, String> {
    if let Some(n) = x.as_f64() {
        return Ok(n);
    }
    if let Some(a) = x.as_array() {
        let lo = a.first().and_then(Value::as_u64).ok_or("bits")?;
        let hi = a.get(1).and_then(Value::as_u64).ok_or("bits")?;
        return Ok(f64::from_bits((hi << 32) | (lo & 0xffff_ffff)));
    }
    match x.as_str() {
        Some("NaN") => Ok(f64::NAN),
        Some("Infinity") => Ok(f64::INFINITY),
        Some("-Infinity") => Ok(f64::NEG_INFINITY),
        _ => Err("expected a number".into()),
    }
}

fn f64_arg(step: &[Value], i: usize) -> Result<f64, String> {
    num_f64(step.get(i).ok_or_else(|| format!("step argument {i}"))?)
}

fn i32_arg(step: &[Value], i: usize) -> Result<i32, String> {
    step.get(i).and_then(Value::as_i64).and_then(|n| i32::try_from(n).ok()).ok_or_else(|| format!("step argument {i}"))
}

fn vec3_arg(step: &[Value], i: usize) -> Result<[f64; 3], String> {
    let v = nums_f64(step.get(i).ok_or_else(|| format!("step argument {i}"))?)?;
    v.try_into().map_err(|_| format!("step argument {i}: three numbers"))
}

fn polys_arg(step: &[Value], i: usize) -> Result<Vec<Vec<[f64; 2]>>, String> {
    let a = step.get(i).and_then(Value::as_array).ok_or_else(|| format!("step argument {i}"))?;
    a.iter()
        .map(|poly| {
            let p = nums_f64(poly)?;
            Ok(p.as_chunks::<2>().0.to_vec())
        })
        .collect()
}

fn s(step: &[Value], i: usize) -> Result<&str, String> {
    step.get(i).and_then(Value::as_str).ok_or_else(|| format!("step argument {i}"))
}

fn run_case(case: &Value, meshes: &[Mesh], emit: bool) -> Result<Vec<Value>, String> {
    // A step naming a handle that does not exist (its mesh failed to construct) is skipped; an output step reports
    // {"missing": true}. mfdiff.mjs does the same on the wasm side.
    let steps = case.get("steps").and_then(Value::as_array).ok_or("steps")?;
    let mut vars: HashMap<String, Manifold> = HashMap::new();
    let mut out = Vec::new();
    for step in steps {
        let step = step.as_array().ok_or("a step is an array")?;
        let op = s(step, 0)?;
        match op {
            "mesh" => {
                let id = s(step, 1)?;
                let i = step.get(2).and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()).ok_or("mesh index")?;
                let merge = step.get(3).and_then(Value::as_bool).unwrap_or(false);
                let mut m = meshes.get(i).cloned().ok_or("mesh index out of range")?;
                if merge {
                    m.merge();
                }
                match Manifold::from_mesh(&m) {
                    Ok(man) => {
                        vars.insert(id.to_string(), man);
                        out.push(json!({ "ctor": "NoError" }));
                    }
                    Err(st) => out.push(json!({ "ctor": st.as_str() })),
                }
            }
            "subtract" | "add" | "intersect" => {
                let r = match (vars.get(s(step, 2)?), vars.get(s(step, 3)?)) {
                    (Some(a), Some(b)) => match op {
                        "subtract" => a.subtract(b),
                        "add" => a.add(b),
                        _ => a.intersect(b),
                    },
                    _ => continue,
                };
                vars.insert(s(step, 1)?.to_string(), r);
            }
            "union" | "intersection" | "difference" => {
                let list = step.get(2).and_then(Value::as_array).ok_or("list")?;
                let ms: Option<Vec<Manifold>> = list.iter().map(|k| k.as_str().and_then(|k| vars.get(k)).cloned()).collect();
                let Some(ms) = ms else { continue };
                let opt = match op {
                    "union" => OpType::Add,
                    "intersection" => OpType::Intersect,
                    _ => OpType::Subtract,
                };
                let r = Manifold::batch_boolean(&ms, opt);
                drop(ms);
                vars.insert(s(step, 1)?.to_string(), r);
            }
            "delete" => {
                vars.remove(s(step, 1)?);
            }
            "cube" => {
                let size = vec3_arg(step, 2)?;
                let center = step.get(3).and_then(Value::as_bool).unwrap_or(false);
                vars.insert(s(step, 1)?.to_string(), Manifold::cube(size, center));
            }
            "sphere" => {
                let r = f64_arg(step, 2)?;
                let seg = i32_arg(step, 3)?;
                vars.insert(s(step, 1)?.to_string(), Manifold::sphere(r, seg));
            }
            "extrude" => {
                let polys = polys_arg(step, 2)?;
                let scale = step.get(6).map(nums_f64).transpose()?.unwrap_or_else(|| vec![1.0, 1.0]);
                let st = [scale.first().copied().unwrap_or(1.0), scale.get(1).copied().unwrap_or(1.0)];
                let m = Manifold::extrude(&polys, f64_arg(step, 3)?, i32_arg(step, 4)?, f64_arg(step, 5)?, st);
                vars.insert(s(step, 1)?.to_string(), m);
            }
            "revolve" => {
                let polys = polys_arg(step, 2)?;
                let m = Manifold::revolve(&polys, i32_arg(step, 3)?, f64_arg(step, 4)?);
                vars.insert(s(step, 1)?.to_string(), m);
            }
            "hullPts" => {
                let pts = nums_f64(step.get(2).ok_or("points")?)?;
                let pts: Vec<[f64; 3]> = pts.as_chunks::<3>().0.to_vec();
                vars.insert(s(step, 1)?.to_string(), Manifold::hull_points(&pts));
            }
            "translate" | "rotate" | "scale" | "mirror" | "transform" | "asOriginal" | "hull" | "trimByPlane" => {
                let Some(a) = vars.get(s(step, 2)?) else { continue };
                let r = match op {
                    "translate" => a.translate(vec3_arg(step, 3)?),
                    "rotate" => {
                        let v = vec3_arg(step, 3)?;
                        a.rotate(v[0], v[1], v[2])
                    }
                    "scale" => a.scale(vec3_arg(step, 3)?),
                    "mirror" => a.mirror(vec3_arg(step, 3)?),
                    "transform" => {
                        let m = nums_f64(step.get(3).ok_or("matrix")?)?;
                        let m: [f64; 16] = m.try_into().map_err(|_| "a 4x4 matrix has 16 numbers".to_string())?;
                        a.transform_mat4(&m)
                    }
                    "asOriginal" => a.as_original(),
                    "hull" => a.hull(),
                    _ => a.trim_by_plane(vec3_arg(step, 3)?, f64_arg(step, 4)?),
                };
                vars.insert(s(step, 1)?.to_string(), r);
            }
            "minkowskiSum" | "minkowskiDifference" => {
                let r = match (vars.get(s(step, 2)?), vars.get(s(step, 3)?)) {
                    (Some(a), Some(b)) => {
                        if op == "minkowskiSum" {
                            a.minkowski_sum(b)
                        } else {
                            a.minkowski_difference(b)
                        }
                    }
                    _ => continue,
                };
                vars.insert(s(step, 1)?.to_string(), r);
            }
            "props" => {
                let Some(m) = vars.get(s(step, 1)?) else {
                    out.push(json!({ "missing": true }));
                    continue;
                };
                let (lo, hi) = m.bounding_box();
                let bbox: Vec<Value> = lo.iter().chain(hi.iter()).map(|&x| f64_bits(x)).collect();
                out.push(json!({
                    "status": m.status().as_str(),
                    "numTri": m.num_tri(),
                    "volume": f64_bits(m.volume()),
                    "area": f64_bits(m.surface_area()),
                    "genus": m.genus(),
                    "originalID": m.original_id(),
                    "bbox": bbox,
                    "decompose": m.decompose_count(),
                }));
            }
            "minGap" => {
                let (Some(a), Some(b)) = (vars.get(s(step, 1)?), vars.get(s(step, 2)?)) else {
                    out.push(json!({ "missing": true }));
                    continue;
                };
                out.push(json!({ "minGap": f64_bits(a.min_gap(b, f64_arg(step, 3)?)) }));
            }
            "get" | "status" | "isEmpty" => {
                let Some(m) = vars.get(s(step, 1)?) else {
                    out.push(json!({ "missing": true }));
                    continue;
                };
                match op {
                    "get" => {
                        let mesh = m.get_mesh();
                        if emit {
                            out.push(json!({ "mesh": mesh_json(&mesh) }));
                        }
                    }
                    "status" => out.push(json!({ "status": m.status().as_str() })),
                    _ => out.push(json!({ "isEmpty": m.is_empty() })),
                }
            }
            _ => return Err(format!("unknown step {op}")),
        }
    }
    Ok(out)
}

pub fn manifold(rest: &[&str]) -> ExitCode {
    let Some(path) = rest.first() else {
        eprintln!("usage: cadcraft-cli manifold <cases.json> [--bench N]");
        return ExitCode::from(2);
    };
    let runs: usize = rest.iter().position(|a| *a == "--bench").and_then(|i| rest.get(i + 1)).and_then(|n| n.parse().ok()).unwrap_or(0);
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    let doc: Value = match serde_json::from_str(&text) {
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
        let name = case.get("name").cloned().unwrap_or(Value::Null);
        let meshes: Result<Vec<Mesh>, String> =
            case.get("meshes").and_then(Value::as_array).map(|a| a.iter().map(parse_mesh).collect()).unwrap_or_else(|| Ok(Vec::new()));
        let mut obj = Map::new();
        obj.insert("name".into(), name);
        match meshes {
            Err(e) => {
                obj.insert("error".into(), Value::from(e));
            }
            Ok(meshes) => {
                match run_case(case, &meshes, true) {
                    Ok(out) => {
                        obj.insert("out".into(), Value::Array(out));
                    }
                    Err(e) => {
                        obj.insert("error".into(), Value::from(e));
                    }
                }
                // malformed inputs are not timed (mfdiff.mjs skips them too)
                let bad = case.get("name").and_then(Value::as_str).is_some_and(|n| n.ends_with("-bad"));
                if runs > 0 && !bad {
                    let mut best = f64::INFINITY;
                    for _ in 0..runs {
                        let t = Instant::now();
                        let _ = run_case(case, &meshes, false);
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
