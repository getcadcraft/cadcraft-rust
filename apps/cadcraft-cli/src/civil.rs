//! `cadcraft-cli civil`: CivilCraft's engine (cc-civil) driven by call scripts — the parity harness's side of
//! forks/tools/cvdiff.mjs.
//!
//!   cadcraft-cli civil <script.json>                      run {calls: [{op, args, as?, self?}]} → [{ok} | {err}]
//!   cadcraft-cli civil --bench <script.json> [--runs N]   ms per run of the whole script, after one warm-up

use std::process::ExitCode;
use std::time::Instant;

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

fn run(v: &serde_json::Value) -> serde_json::Value {
    cc_civil::script::run_with(v, &[cc_io_civil::script::handle, cc_civil::script::handle, view_ops])
}

/// ModelCraft's camera (cc-m3d-tools) as the 3D site view's picking camera.
struct Cam(cc_m3d_tools::camera::Camera);
impl cc_civil::site3d::PickCamera for Cam {
    fn ray(&self, sx: f64, sy: f64) -> ([f64; 3], [f64; 3]) {
        let r = self.0.ray(sx, sy);
        (r.o, r.d)
    }
    fn view_proj(&self) -> [f64; 16] {
        self.0.view_proj()
    }
    fn project_vp(&self, p: [f64; 3], vp: &[f64; 16]) -> Option<[f64; 3]> {
        self.0.project_vp(p, vp)
    }
}

/// `new Camera(); setSize(w, h); standard(view, scene.bbox); persp = …` from `{w, h, view, persp}`.
fn cam_of(sc: &cc_civil::site3d::Scene, o: Option<&serde_json::Value>) -> Cam {
    let g = |k: &str| o.and_then(|o| o.get(k));
    let mut cam = cc_m3d_tools::camera::Camera::new();
    cam.set_size(g("w").and_then(serde_json::Value::as_f64).unwrap_or(800.0), g("h").and_then(serde_json::Value::as_f64).unwrap_or(600.0));
    let bb = sc.bbox.map(|(min, max)| cc_m3d_tools::camera::Box3 { min, max });
    cam.standard(g("view").and_then(serde_json::Value::as_str).unwrap_or("iso"), bb.as_ref());
    if g("persp").and_then(serde_json::Value::as_bool) == Some(false) {
        cam.persp = false;
    }
    Cam(cam)
}

/// The view ops that need the camera (a layer above cc-civil): `s3.pick`, `s3.declutter`.
fn view_ops(reg: &mut cc_civil::script::Registry, c: &cc_civil::script::Call) -> Option<Result<serde_json::Value, String>> {
    use cc_civil::js::J;
    use cc_civil::site3d::{PickCamera, Scene, declutter_labels, pick_site};
    if c.op.starts_with("bench.") {
        return bench_ops(reg, c);
    }
    if c.op != "s3.pick" && c.op != "s3.declutter" {
        return None;
    }
    let name = c.arg(0).and_then(serde_json::Value::as_str).unwrap_or("");
    let Some(sc) = reg.get::<Scene>(name) else { return Some(Err(format!("no object {name}"))) };
    let cam = cam_of(sc, c.arg(1));
    if c.op == "s3.pick" {
        let tol = c.arg(4).and_then(serde_json::Value::as_f64).unwrap_or(8.0);
        return Some(Ok(pick_site(sc, &cam, c.num(2), c.num(3), tol).map_or(serde_json::Value::Null, |p| p.j())));
    }
    let vp = cam.view_proj();
    let o = c.arg(1);
    let f = |k: &str, d: f64| o.and_then(|o| o.get(k)).and_then(serde_json::Value::as_f64).unwrap_or(d);
    let vis = declutter_labels(&sc.labels, &|p| cam.project_vp(p, &vp), f("w", 800.0), f("h", 600.0), 16.0, 7.0, 2.0);
    Some(Ok(serde_json::Value::Array(vis.into_iter().map(serde_json::Value::from).collect())))
}

pub fn civil(rest: &[&str]) -> ExitCode {
    let flag = |k: &str| rest.iter().position(|a| *a == k);
    let value_of = |k: &str| flag(k).and_then(|i| rest.get(i + 1)).copied();
    if let Some(file) = value_of("--bench") {
        let v = match read_json(file) {
            Ok(v) => v,
            Err(c) => return c,
        };
        let runs: usize = value_of("--runs").and_then(|s| s.parse().ok()).unwrap_or(5).clamp(1, 1000);
        drop(run(&v));
        let mut ms = Vec::with_capacity(runs);
        for _ in 0..runs {
            let t = Instant::now();
            let r = run(&v);
            ms.push(t.elapsed().as_secs_f64() * 1000.0);
            drop(r);
        }
        let best = ms.iter().copied().fold(f64::INFINITY, f64::min);
        let mut sorted = ms.clone();
        sorted.sort_by(f64::total_cmp);
        let median = sorted.get(sorted.len() / 2).copied().unwrap_or(best);
        println!("{}", serde_json::json!({ "runs": ms, "best": best, "median": median }));
        return ExitCode::SUCCESS;
    }
    let Some(file) = rest.iter().find(|a| !a.starts_with("--")) else {
        eprintln!("usage: cadcraft-cli civil <script.json> | --bench <script.json> [--runs N]");
        return ExitCode::from(2);
    };
    let v = match read_json(file) {
        Ok(v) => v,
        Err(c) => return c,
    };
    println!("{}", run(&v));
    ExitCode::SUCCESS
}

/// mulberry32 (the harness's seeded generator).
fn mulberry(seed: u32) -> impl FnMut() -> f64 {
    let mut a = seed;
    move || {
        a = a.wrapping_add(0x6d2b_79f5);
        let mut t = (a ^ (a >> 15)).wrapping_mul(a | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        f64::from(t ^ (t >> 14)) / 4_294_967_296.0
    }
}

/// Speed-table ops (`--bench` scripts; forks/tools/cvbench.mjs has the JS twins): `bench.tin [n, seed, ox, oy, size, dz]`
/// keeps a surface of n seeded points on a rolling terrain; `bench.lxFile [path]` reads a LandXML file and builds its
/// first surface (the import path).
fn bench_ops(reg: &mut cc_civil::script::Registry, c: &cc_civil::script::Call) -> Option<Result<serde_json::Value, String>> {
    match c.op {
        "bench.tin" => {
            let n = c.arg(0).and_then(serde_json::Value::as_u64).unwrap_or(1000).min(5_000_000);
            let seed = u32::try_from(c.arg(1).and_then(serde_json::Value::as_u64).unwrap_or(1)).unwrap_or(1);
            let (ox, oy, size, dz) = (c.num(2), c.num(3), c.num(4), c.num(5));
            let mut r = mulberry(seed);
            let mut pts = Vec::with_capacity(usize::try_from(n).unwrap_or(0));
            for _ in 0..n {
                let (u, v) = (r() * size, r() * size);
                let z = 100.0 + 10.0 * (u / 50.0).sin() + 5.0 * (v / 37.0).cos() + r() * 0.5 + dz;
                pts.push(cc_civil::tin::ZPt { x: ox + u, y: oy + v, z: Some(z) });
            }
            let b = cc_civil::tin::build_surface(&pts, &[], None, 0.0);
            let nt = b.surface.as_ref().map_or(0, |s| s.n_t);
            if let (Some(name), Some(s)) = (c.as_name, b.surface) {
                reg.put(name, s);
            }
            Some(Ok(serde_json::json!({ "nT": nt })))
        }
        "bench.gridVolume" | "bench.contours" => {
            // the best of `runs` timings of one engine call on kept surfaces (ms), so surface building is not counted
            let runs = c.arg(2).and_then(serde_json::Value::as_u64).unwrap_or(3).clamp(1, 50);
            let Some(a) = c.arg(0).and_then(serde_json::Value::as_str).and_then(|n| reg.get::<cc_civil::tin::Surface>(n)) else {
                return Some(Err("no surface".into()));
            };
            let mut best = f64::INFINITY;
            for _ in 0..=runs {
                let t = Instant::now();
                if c.op == "bench.contours" {
                    std::hint::black_box(a.contours(c.num(1), 0.0, 2000.0));
                } else {
                    let Some(b) = c.arg(1).and_then(serde_json::Value::as_str).and_then(|n| reg.get::<cc_civil::tin::Surface>(n)) else {
                        return Some(Err("no surface".into()));
                    };
                    std::hint::black_box(cc_civil::grading::grid_volume(a, b, f64::NAN, None, None, 4e6));
                }
                best = best.min(t.elapsed().as_secs_f64() * 1000.0);
            }
            Some(Ok(serde_json::json!({ "ms": best })))
        }
        "bench.lxFile" => {
            let path = c.str(0);
            let text = match std::fs::read_to_string(path) {
                Ok(t) => t,
                Err(e) => return Some(Err(e.to_string())),
            };
            let d = match cc_io_civil::landxml::read_land_xml(&text) {
                Ok(d) => d,
                Err(e) => return Some(Err(e)),
            };
            let s0 = d.get("surfaces").and_then(|s| s.get(0)).cloned().unwrap_or(serde_json::Value::Null);
            let pts: Vec<Option<cc_civil::geo::P3>> = cc_civil::js::arr(s0.get("pts"))
                .iter()
                .map(|p| if p.is_object() { Some(cc_civil::geo::read_p3(p)) } else { None })
                .collect();
            let faces: Vec<Vec<f64>> = cc_civil::js::arr(s0.get("faces"))
                .iter()
                .map(|f| cc_civil::js::arr(Some(f)).iter().map(|x| cc_civil::js::numf(Some(x))).collect())
                .collect();
            let b = cc_civil::tin::surface_from_faces(&pts, &faces);
            Some(Ok(
                serde_json::json!({ "points": cc_civil::js::arr(d.get("points")).len(), "nT": b.surface.as_ref().map_or(0, |s| s.n_t) }),
            ))
        }
        _ => None,
    }
}
