//! The CLI's `parity <engine> <file>` output against the CadCraft 0.5 JavaScript references of the public fixtures
//! (`tests/golden/`, made by the parity kit's `ref.mjs --public-out`), compared as the kit compares: keys in any order,
//! -0 = 0, numbers equal when |a − b| ≤ abs + rel·max(|a|, |b|). An engine the CLI has not ported yet (exit 3) is skipped
//! with a note; the full corpus check is the kit's `run.mjs` on the build server.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden")
}

fn gunzip(p: &Path) -> String {
    let mut s = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(p).unwrap()).read_to_string(&mut s).unwrap();
    s
}

/// The kit's volatile fields: left out of the references.
fn strip(engine: &str, v: &mut Value) {
    if engine == "dwg"
        && let Some(r) = v.get_mut("report").and_then(Value::as_object_mut)
    {
        r.remove("ms");
    }
}

fn same(a: &Value, b: &Value, abs: f64, rel: f64, path: &str, out: &mut Vec<String>) {
    if out.len() >= 20 {
        return;
    }
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
            if x != y && (x - y).abs() > abs + rel * x.abs().max(y.abs()) {
                out.push(format!("{path}: {x} vs {y}"));
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                out.push(format!("{path}: length {} vs {}", x.len(), y.len()));
                return;
            }
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                same(p, q, abs, rel, &format!("{path}[{i}]"), out);
            }
        }
        (Value::Object(x), Value::Object(y)) => {
            for (k, p) in x {
                // the kit's canon drops `undefined`; serde has no undefined, but null-vs-missing is a real difference
                match y.get(k) {
                    Some(q) => same(p, q, abs, rel, &format!("{path}.{k}"), out),
                    None => out.push(format!("{path}.{k}: only in ours")),
                }
            }
            for k in y.keys().filter(|k| !x.contains_key(*k)) {
                out.push(format!("{path}.{k}: only in the reference"));
            }
        }
        _ if a == b => {}
        _ => out.push(format!("{path}: {a} vs {b}")),
    }
}

fn check(engine: &str, abs: f64, rel: f64, ndjson: bool) {
    let dir = root().join("js-0.5").join(engine);
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
    files.sort();
    assert!(!files.is_empty(), "no references in {}", dir.display());
    let mut failures = Vec::new();
    for gz in files {
        let name = gz.file_name().unwrap().to_string_lossy().to_string();
        let fixture = name.trim_end_matches(".gz").trim_end_matches(".json").trim_end_matches(".ndjson").to_string();
        let src =
            [root().join("fixtures").join(&fixture), root().join("../../assets/samples").join(&fixture)].into_iter().find(|p| p.exists());
        let Some(src) = src else {
            failures.push(format!("{engine} {fixture}: fixture missing"));
            continue;
        };
        let run = Command::new(env!("CARGO_BIN_EXE_cadcraft-cli")).args(["parity", engine]).arg(&src).output().unwrap();
        if run.status.code() == Some(3) {
            eprintln!("{engine}: not ported yet, {fixture} skipped");
            continue;
        }
        let ours = String::from_utf8(run.stdout).unwrap();
        let refs = gunzip(&gz);
        let parse = |t: &str| -> Vec<Value> {
            if ndjson {
                t.lines().filter(|l| !l.is_empty()).map(|l| serde_json::from_str(l).unwrap()).collect()
            } else {
                vec![serde_json::from_str(t).unwrap()]
            }
        };
        let (mut a, b) = (parse(&ours), parse(&refs));
        for v in &mut a {
            strip(engine, v);
        }
        let mut diffs = Vec::new();
        same(&Value::Array(a), &Value::Array(b), abs, rel, "", &mut diffs);
        if !diffs.is_empty() {
            failures.push(format!("{engine} {fixture}:\n  {}", diffs.join("\n  ")));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn dwg_matches_the_js_references() {
    check("dwg", 1e-9, 1e-12, false);
}

#[test]
fn ifc_step_matches_the_js_references() {
    check("ifc-step", 0.0, 0.0, true);
}

#[test]
fn ifc_geom_matches_the_js_references() {
    check("ifc-geom", 1e-6, 1e-6, false);
}
