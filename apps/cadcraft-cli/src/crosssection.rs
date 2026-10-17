//! `cadcraft-cli manifold-cs <cases.json> [--bench N]`: runs CrossSection scripts through cc-manifold and prints each
//! output as JSON, for the parity harness `forks/tools/csdiff.mjs` (which runs the same scripts through manifold-3d's
//! wasm). Every float travels as its IEEE bit pattern in 16 hex digits (exact, -0 and NaN included).
//!
//! ```text
//! ["new", "A", [[[x, y], ...], ...], fill]     A = new CrossSection(polygons, fill)   fill: EvenOdd|NonZero|Positive|Negative
//! ["circle", "A", r, n] | ["square", "A", [w, h], center]
//! ["add"|"subtract"|"intersect", "R", "A", "B"]
//! ["union"|"difference"|"intersection", "R", ["A", "B", ...]]    CrossSection.union(list) etc.
//! ["translate"|"scale"|"mirror", "R", "A", [x, y]] | ["rotate", "R", "A", deg] | ["transform", "R", "A", [9 numbers]]
//! ["out", "A"]      {polys, area, isEmpty, numVert, numContour, bounds}
//! ```
//! A step whose operation throws in the wasm (Clipper2's range error) outputs `{"throw": true}` and leaves its handle
//! unset; steps naming an unset handle are skipped (`out` reports `{"missing": true}`).

use std::collections::HashMap;
use std::process::ExitCode;
use std::time::Instant;

use cc_manifold::{ClipError, CrossSection, FillRule, OpType};
use serde_json::{Map, Value, json};

fn hexf(v: &Value) -> Result<f64, String> {
    let s = v.as_str().ok_or("expected a hex float")?;
    u64::from_str_radix(s, 16).map(f64::from_bits).map_err(|e| e.to_string())
}

fn bitsf(f: f64) -> Value {
    Value::from(format!("{:016x}", f.to_bits()))
}

fn pt(v: &Value) -> Result<[f64; 2], String> {
    let a = v.as_array().ok_or("a point is an array")?;
    Ok([hexf(a.first().ok_or("x")?)?, hexf(a.get(1).ok_or("y")?)?])
}

fn polys(v: &Value) -> Result<Vec<Vec<[f64; 2]>>, String> {
    v.as_array().ok_or("polygons")?.iter().map(|p| p.as_array().ok_or("polygon")?.iter().map(pt).collect()).collect()
}

fn fill(s: &str) -> Result<FillRule, String> {
    Ok(match s {
        "EvenOdd" => FillRule::EvenOdd,
        "NonZero" => FillRule::NonZero,
        "Positive" => FillRule::Positive,
        "Negative" => FillRule::Negative,
        _ => return Err(format!("fill rule {s}")),
    })
}

fn s(step: &[Value], i: usize) -> Result<&str, String> {
    step.get(i).and_then(Value::as_str).ok_or_else(|| format!("step argument {i}"))
}

fn out_json(cs: &CrossSection) -> Value {
    let polys: Vec<Value> =
        cs.to_polygons().iter().map(|p| Value::Array(p.iter().map(|q| json!([bitsf(q[0]), bitsf(q[1])])).collect())).collect();
    let (mn, mx) = cs.bounds();
    json!({
        "polys": polys,
        "area": bitsf(cs.area()),
        "isEmpty": cs.is_empty(),
        "numVert": cs.num_vert(),
        "numContour": cs.num_contour(),
        "bounds": [bitsf(mn[0]), bitsf(mn[1]), bitsf(mx[0]), bitsf(mx[1])],
    })
}

fn run_case(case: &Value, emit: bool) -> Result<Vec<Value>, String> {
    let steps = case.get("steps").and_then(Value::as_array).ok_or("steps")?;
    let mut vars: HashMap<String, CrossSection> = HashMap::new();
    let mut out = Vec::new();
    for step in steps {
        let step = step.as_array().ok_or("a step is an array")?;
        let op = s(step, 0)?;
        let id = s(step, 1)?.to_string();
        let r: Result<CrossSection, ClipError> = match op {
            "new" => CrossSection::try_new(&polys(step.get(2).ok_or("polygons")?)?, fill(s(step, 3)?)?),
            "circle" => {
                let n = step.get(3).and_then(Value::as_i64).and_then(|n| i32::try_from(n).ok()).ok_or("segments")?;
                Ok(CrossSection::circle(hexf(step.get(2).ok_or("radius")?)?, n))
            }
            "square" => Ok(CrossSection::square(pt(step.get(2).ok_or("size")?)?, step.get(3).and_then(Value::as_bool).unwrap_or(false))),
            "add" | "subtract" | "intersect" => {
                let (Some(a), Some(b)) = (vars.get(s(step, 2)?), vars.get(s(step, 3)?)) else { continue };
                let o = match op {
                    "add" => OpType::Add,
                    "subtract" => OpType::Subtract,
                    _ => OpType::Intersect,
                };
                a.try_boolean(b, o)
            }
            "union" | "difference" | "intersection" => {
                let list = step.get(2).and_then(Value::as_array).ok_or("list")?;
                let cs: Option<Vec<CrossSection>> = list.iter().map(|k| k.as_str().and_then(|k| vars.get(k)).cloned()).collect();
                let Some(cs) = cs else { continue };
                let o = match op {
                    "union" => OpType::Add,
                    "difference" => OpType::Subtract,
                    _ => OpType::Intersect,
                };
                CrossSection::try_batch_boolean(&cs, o)
            }
            "translate" | "scale" | "mirror" | "rotate" | "transform" => {
                let Some(a) = vars.get(s(step, 2)?) else { continue };
                let arg = step.get(3).ok_or("argument")?;
                Ok(match op {
                    "translate" => a.translate(pt(arg)?),
                    "scale" => a.scale(pt(arg)?),
                    "mirror" => a.mirror(pt(arg)?),
                    "rotate" => a.rotate(hexf(arg)?),
                    _ => {
                        let m: Vec<f64> = arg.as_array().ok_or("matrix")?.iter().map(hexf).collect::<Result<_, _>>()?;
                        let g = |i: usize| m.get(i).copied().ok_or("matrix entry");
                        // the binding reads matrix[col][row] = array[col * 3 + row]
                        a.transform([g(0)?, g(1)?, g(3)?, g(4)?, g(6)?, g(7)?])
                    }
                })
            }
            "out" => {
                match vars.get(&id) {
                    Some(cs) => {
                        if emit {
                            out.push(out_json(cs));
                        }
                    }
                    None => out.push(json!({ "missing": true })),
                }
                continue;
            }
            _ => return Err(format!("unknown step {op}")),
        };
        match r {
            Ok(cs) => {
                vars.insert(id, cs);
            }
            Err(_) => {
                vars.remove(&id);
                out.push(json!({ "throw": true }));
            }
        }
    }
    Ok(out)
}

pub fn run(rest: &[&str]) -> ExitCode {
    let Some(path) = rest.first() else {
        eprintln!("usage: cadcraft-cli manifold-cs <cases.json> [--bench N]");
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
        match run_case(case, true) {
            Ok(out) => {
                obj.insert("out".into(), Value::Array(out));
            }
            Err(e) => {
                obj.insert("error".into(), Value::from(e));
            }
        }
        if runs > 0 {
            let mut best = f64::INFINITY;
            for _ in 0..runs {
                let t = Instant::now();
                let _ = run_case(case, false);
                best = best.min(t.elapsed().as_secs_f64() * 1000.0);
            }
            obj.insert("ms".into(), Value::from(best));
        }
        results.push(Value::Object(obj));
    }
    println!("{}", json!({ "cases": results }));
    ExitCode::SUCCESS
}
