//! `cadcraft-cli m3dtools`: ModelCraft's interaction layer (cc-m3d-tools: camera, inference, picking, collisions,
//! the Tools) driven by scripted sessions — the parity harness's side of forks/tools/tdiff.mjs.
//!
//!   cadcraft-cli m3dtools <script.json>            run a session; after each step print the model JSON, the tool's
//!                                                  state (status / tip / inference kind + point) and the overlay
//!   cadcraft-cli m3dtools --bench <bench.json> [--runs N]
//!                                                  speed: {init (model), cam, points: [[x, y]…]} → ms to build the
//!                                                  scene, to infer at every point and to pick at every point (best
//!                                                  of N, after one warm-up)

use std::process::ExitCode;
use std::time::Instant;

use cc_m3d_tools::camera::Camera;
use cc_m3d_tools::infer::{InferOpts, infer, pick};
use cc_m3d_tools::scene::{LocalCache, Scene};

fn read_json(path: &str) -> Result<serde_json::Value, ExitCode> {
    let bytes = std::fs::read(path).map_err(|e| {
        crate::print_error("io", &e.to_string());
        ExitCode::from(1)
    })?;
    serde_json::from_slice(&bytes).map_err(|e| {
        crate::print_error("json", &e.to_string());
        ExitCode::from(1)
    })
}

fn bench(v: &serde_json::Value, runs: usize) -> Result<serde_json::Value, String> {
    let mut m = cc_m3d::Model::new();
    if let Some(init) = v.get("init") {
        m.load(init, false).map_err(|e| e.to_string())?;
    }
    let mut cam = Camera::new();
    if let Some(c) = v.get("cam") {
        cc_m3d_tools::session::set_cam(&mut cam, c);
    }
    let pts: Vec<(f64, f64)> = v
        .get("points")
        .and_then(serde_json::Value::as_array)
        .map(|a| {
            a.iter()
                .map(|p| {
                    (
                        p.get(0).and_then(serde_json::Value::as_f64).unwrap_or(0.0),
                        p.get(1).and_then(serde_json::Value::as_f64).unwrap_or(0.0),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let (mut best_scene, mut best_infer, mut best_pick) = (f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut kinds = 0usize;
    for run in 0..=runs {
        let t0 = Instant::now();
        let mut cache = LocalCache::new();
        let sc = Scene::of(&m, &mut cache);
        let t1 = Instant::now();
        let opts = InferOpts { radius: Some(12.0), ..InferOpts::default() };
        let mut n = 0usize;
        for &(x, y) in &pts {
            let r = infer(&sc, &cam, x, y, &opts);
            if r.kind != "none" {
                n += 1;
            }
        }
        let t2 = Instant::now();
        for &(x, y) in &pts {
            let _ = pick(&sc, &cam, x, y, &[], 7.0);
        }
        let t3 = Instant::now();
        kinds = n;
        if run > 0 {
            best_scene = best_scene.min((t1 - t0).as_secs_f64() * 1000.0);
            best_infer = best_infer.min((t2 - t1).as_secs_f64() * 1000.0);
            best_pick = best_pick.min((t3 - t2).as_secs_f64() * 1000.0);
        }
    }
    Ok(serde_json::json!({"scene": best_scene, "infer": best_infer, "pick": best_pick, "points": pts.len(), "snapped": kinds}))
}

pub fn m3dtools(rest: &[&str]) -> ExitCode {
    let flag = |k: &str| rest.iter().position(|a| *a == k);
    let value_of = |k: &str| flag(k).and_then(|i| rest.get(i + 1)).copied();
    if let Some(file) = value_of("--bench") {
        let v = match read_json(file) {
            Ok(v) => v,
            Err(c) => return c,
        };
        let runs: usize = value_of("--runs").and_then(|s| s.parse().ok()).unwrap_or(5).clamp(1, 1000);
        return match bench(&v, runs) {
            Ok(r) => {
                println!("{r}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                crate::print_error("bench", &e);
                ExitCode::from(1)
            }
        };
    }
    let Some(file) = rest.iter().find(|a| !a.starts_with("--")) else {
        eprintln!("usage: cadcraft-cli m3dtools <script.json> | --bench <bench.json> [--runs N]");
        return ExitCode::from(2);
    };
    let v = match read_json(file) {
        Ok(v) => v,
        Err(c) => return c,
    };
    match cc_m3d_tools::session::run(&v, true) {
        Ok(s) => {
            println!("{s}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            crate::print_error("script", &e.to_string());
            ExitCode::from(1)
        }
    }
}
