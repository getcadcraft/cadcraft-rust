//! cc-civil against values worked by hand in the frozen 0.5 tests (forks/work/ref/test-civilcraft*.mjs), driven through
//! the call-script runner the parity harness uses. Fixtures are generated here; no corpus or client files.

use serde_json::{Value, json};

fn run(calls: Value) -> Vec<Value> {
    let out = cc_civil::script::run(&json!({ "calls": calls }));
    out.as_array().cloned().unwrap_or_default()
}
fn ok(v: &Value) -> &Value {
    assert!(v.get("err").is_none(), "call failed: {v}");
    &v["ok"]
}
fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or(f64::NAN)
}
fn near(a: f64, b: f64, tol: f64, what: &str) {
    assert!((a - b).abs() <= tol, "{what}: {a} vs {b} (±{tol})");
}
#[allow(clippy::too_many_arguments)]
fn grid(i0: i32, i1: i32, j0: i32, j1: i32, step: f64, ox: f64, oy: f64, z: impl Fn(f64, f64) -> f64) -> Value {
    let mut p = Vec::new();
    for i in i0..=i1 {
        for j in j0..=j1 {
            let (x, y) = (f64::from(i) * step, f64::from(j) * step);
            p.push(json!({ "x": x + ox, "y": y + oy, "z": z(x, y) }));
        }
    }
    Value::Array(p)
}

#[test]
fn crest_curve_worked_by_hand() {
    let r = run(json!([
        { "op": "profile.build", "args": [[{ "sta": 0, "z": 100 }, { "sta": 500, "z": 110, "L": 200 }, { "sta": 1000, "z": 100 }]], "as": "P" },
        { "op": "prof.elevationAt", "self": "P", "args": [500] },
        { "op": "prof.elevationAt", "self": "P", "args": [400] },
        { "op": "prof.gradeAt", "self": "P", "args": [500] },
    ]));
    // A = 4 %, L = 200: the PVI drops A·L/8 = 1.0
    near(f(ok(&r[1])), 109.0, 1e-9, "elevation at the PVI");
    near(f(ok(&r[2])), 108.0, 1e-9, "at the BVC: on the tangent");
    near(f(ok(&r[3])), 0.0, 1e-12, "grade at the crest");
}

#[test]
fn curve_length_and_stations() {
    let r = run(json!([
        { "op": "alignment.build", "args": [{ "pis": [{ "x": 0, "y": 0 }, { "x": 500, "y": 0, "r": 200 }, { "x": 500, "y": 600 }], "start": 1000 }], "as": "A" },
        { "op": "al.stationOffset", "self": "A", "args": [150, -10] },
    ]));
    let a = ok(&r[0]);
    // T = R tan(Δ/2) = 200; L = R Δ = 100 π; total = (500 − 200) + 100 π + (600 − 200)
    near(f(&a["length"]), 300.0 + 100.0 * std::f64::consts::PI + 400.0, 1e-9, "length");
    near(f(&a["end"]), 1000.0 + f(&a["length"]), 1e-9, "end station");
    let so = ok(&r[1]);
    near(f(&so["sta"]), 1150.0, 1e-9, "station");
    near(f(&so["off"]), 10.0, 1e-9, "offset right of an eastbound line");
}

#[test]
fn pad_graded_into_flat_ground() {
    let pts = grid(-10, 10, -10, 10, 10.0, 0.0, 0.0, |_, _| 0.0);
    let pad = json!({ "pts": [{ "x": -10, "y": -10 }, { "x": 10, "y": -10 }, { "x": 10, "y": 10 }, { "x": -10, "y": 10 }], "z": 2, "cut": 2, "fill": 2 });
    let r = run(json!([
        { "op": "tin.buildSurface", "args": [{ "points": pts }], "as": "F" },
        { "op": "grading.gradedSurface", "args": ["F", pts, [], [pad]], "as": "G" },
        { "op": "surface.elevationAt", "self": "G", "args": [3, -4] },
        { "op": "surface.elevationAt", "self": "G", "args": [12, 0] },
        { "op": "surface.elevationAt", "self": "G", "args": [30, 30] },
    ]));
    assert!(ok(&r[1])["surface"].is_object(), "graded surface built");
    near(f(ok(&r[2])), 2.0, 1e-9, "flat on the pad");
    near(f(ok(&r[3])), 1.0, 1e-6, "half way down the 2:1 slope");
    near(f(ok(&r[4])), 0.0, 1e-9, "ground beyond the daylight line");
}

#[test]
fn corridor_on_flat_ground() {
    let pts = grid(-5, 25, -8, 8, 10.0, 0.0, 0.0, |_, _| 100.0);
    let asm = json!({ "lane": 12, "laneSlope": 0.02, "shoulder": 4, "shoulderSlope": 0.04, "cut": 2, "fill": 3, "step": 25 });
    let r = run(json!([
        { "op": "tin.buildSurface", "args": [{ "points": pts }], "as": "F" },
        { "op": "alignment.build", "args": [{ "pis": [{ "x": 0, "y": 0 }, { "x": 200, "y": 0 }] }], "as": "A" },
        { "op": "profile.build", "args": [[{ "sta": 0, "z": 103 }, { "sta": 200, "z": 103 }]], "as": "P" },
        { "op": "grading.gradedSurface", "args": ["F", pts, [], [{ "name": "c", "corridor": { "alignment": "A", "profile": "P", "asm": asm } }]], "as": "G" },
        { "op": "surface.elevationAt", "self": "G", "args": [100, 0] },
        { "op": "surface.elevationAt", "self": "G", "args": [100, -6] },
        { "op": "surface.elevationAt", "self": "G", "args": [100, 40] },
    ]));
    near(f(ok(&r[4])), 103.0, 1e-9, "centreline at the profile");
    near(f(ok(&r[5])), 102.88, 1e-9, "half way across the lane");
    near(f(ok(&r[6])), 100.0, 1e-9, "ground beyond the daylight");
}

#[test]
fn survey_coordinates_keep_their_precision() {
    // a plane at UTM / state-plane magnitudes: every interpolated elevation exact to 1e-9
    let (ox, oy) = (500_000.125, 4_000_000.75);
    let plane = |x: f64, y: f64| 100.0 + 0.02 * x - 0.03 * y;
    let pts = grid(0, 20, 0, 20, 25.0, ox, oy, plane);
    let mut calls = vec![json!({ "op": "tin.buildSurface", "args": [{ "points": pts }], "as": "S" })];
    let probes: Vec<(f64, f64)> = (0..50).map(|k| (f64::from(k) * 9.73 + 3.1, f64::from(k * 7 % 50) * 9.1 + 1.7)).collect();
    for (x, y) in &probes {
        calls.push(json!({ "op": "surface.elevationAt", "self": "S", "args": [ox + x, oy + y] }));
    }
    let r = run(Value::Array(calls));
    for (k, (x, y)) in probes.iter().enumerate() {
        near(f(ok(&r[k + 1])), plane(*x, *y), 1e-9, "elevation on the plane");
    }
}

#[test]
fn site3d_local_origin_at_survey_scale() {
    let (ox, oy) = (6_150_000.0, 2_080_000.0);
    let pts: Vec<Value> = (0..200)
        .map(|i| {
            let (u, v) = (f64::from(i % 20) * 20.0 + 0.37, f64::from(i / 20) * 30.0 + 0.61);
            json!({ "n": i.to_string(), "x": ox + u, "y": oy + v, "z": 250.0 + (u / 40.0).sin() * 3.0, "d": "GRD" })
        })
        .collect();
    let r = run(json!([
        { "op": "project.new", "as": "P" },
        { "op": "project.setPoints", "self": "P", "args": [pts, "t.csv"] },
        { "op": "s3.input", "args": ["P"], "as": "I" },
        { "op": "s3.build", "args": ["I", { "exag": 1, "color": "single" }], "as": "S" },
    ]));
    let sc = ok(&r[3]);
    let o = &sc["origin"];
    assert!(f(&o["x"]) > 6e6 && f(&o["y"]) > 2e6, "origin at the survey coordinates");
    let pos = sc["meshes"][0]["positions"].as_array().cloned().unwrap_or_default();
    assert!(!pos.is_empty());
    let mut worst_local = 0.0f64;
    for p in pos.chunks(3) {
        worst_local = worst_local.max(f(&p[0]).abs()).max(f(&p[1]).abs());
        // back to project coordinates within a millimetre-scale tolerance (f32 of a small local value)
        let x = f(&p[0]) + f(&o["x"]);
        assert!((x - ox).abs() < 500.0, "x back near the site");
    }
    assert!(worst_local < 1000.0, "local coordinates stay small ({worst_local})");
}

#[test]
fn undo_and_project_file() {
    let pts = grid(0, 8, 0, 8, 10.0, 1000.0, 2000.0, |x, y| 50.0 + x * 0.01 + y * 0.02);
    let pts: Vec<Value> = pts
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(i, mut p)| {
            p["n"] = json!((i + 1).to_string());
            p["d"] = json!("GRD");
            p
        })
        .collect();
    let r = run(json!([
        { "op": "project.new", "as": "P" },
        { "op": "project.setPoints", "self": "P", "args": [pts, "a.csv"] },
        { "op": "project.view", "self": "P" },
        { "op": "project.change", "self": "P", "args": ["Name", [{ "set": ["name"], "value": "Site" }]] },
        { "op": "project.undo", "self": "P" },
        { "op": "project.view", "self": "P" },
        { "op": "project.reload", "self": "P", "args": [[]], "as": "Q" },
        { "op": "project.eg", "self": "Q" },
    ]));
    assert_eq!(ok(&r[2])["state"], ok(&r[5])["state"], "undo restores the state");
    assert!(ok(&r[7])["surface"].is_object(), "the project file reloads and builds its surface");
}

#[test]
fn traverse_and_parcel() {
    let r = run(json!([
        { "op": "tv.run", "args": [{ "start": { "x": 0, "y": 0 }, "mode": "bearings", "legsText": "N 0 E 100 B\nN 90 E 100 C\nS 0 E 100 D\nS 90 W 100", "close": "loop" }] },
        { "op": "pc.area", "args": [[{ "x": 0, "y": 0 }, { "x": 100, "y": 0 }, { "x": 100, "y": 100 }, { "x": 0, "y": 100 }]] },
        { "op": "pc.parseBearing", "args": ["N 45°30'15\" E"] },
    ]));
    let tv = ok(&r[0]);
    assert!(tv.is_object());
    near(f(ok(&r[1])).abs(), 10_000.0, 1e-9, "square parcel area");
    near(f(ok(&r[2])).to_degrees(), 90.0 - (45.0 + 30.0 / 60.0 + 15.0 / 3600.0), 1e-9, "bearing as a math angle (radians)");
}

#[test]
fn renumber_on_paste() {
    let p = |n: &str| json!({ "n": n, "x": 0, "y": 0, "z": 1, "d": "GRD" });
    let r = run(json!([{ "op": "clip.renumber", "args": [[p("1"), p("2"), p("3")], [p("1"), p("2"), p("10"), p("CP1")]] }]));
    let o = ok(&r[0]);
    let ns: Vec<&str> = o["points"].as_array().map(|a| a.iter().filter_map(|q| q["n"].as_str()).collect()).unwrap_or_default();
    assert_eq!(ns, ["11", "12", "3"], "clashing numbers continue after the highest; free ones stay");
    assert_eq!(o["kept"], json!(1));
}

#[test]
fn agent_tools_describe_the_site() {
    let pts: Vec<Value> = (0..30).map(|i| json!({ "n": (i + 1).to_string(), "x": 100.0 + f64::from(i % 6) * 10.0, "y": 200.0 + f64::from(i / 6) * 10.0, "z": 10.0 + f64::from(i) * 0.1, "d": if i < 4 { "TREE OAK" } else { "GRD" } })).collect();
    let r = run(json!([
        { "op": "project.new", "as": "P" },
        { "op": "project.setPoints", "self": "P", "args": [pts, "a.csv"] },
        { "op": "agent.run", "args": ["P", "site_describe", {}] },
        { "op": "agent.run", "args": ["P", "points_list", { "group": "tree" }] },
        { "op": "agent.run", "args": ["P", "points_select", { "numbers": ["1", "NOPE"] }] },
        { "op": "agent.tools" },
    ]));
    let d = &ok(&r[2])["result"];
    assert_eq!(d["points"]["count"], json!(30));
    assert_eq!(ok(&r[3])["result"]["total"], json!(4));
    assert!(r[4]["err"].as_str().is_some_and(|e| e.contains("NOPE")));
    assert_eq!(ok(&r[5]).as_array().map(Vec::len), Some(5));
}

/// Hostile input never panics: random call scripts over every op family (a deterministic generator).
#[test]
fn hostile_calls_do_not_panic() {
    let ops = [
        "units.formatStation",
        "units.parseStation",
        "points.parsePoints",
        "tin.buildSurface",
        "alignment.build",
        "profile.build",
        "grading.daylightPad",
        "corridor.patch",
        "pipes.analyze",
        "hydro.flows",
        "sec.cutFill",
        "fl.offset",
        "dk.figures",
        "pc.parseCalls",
        "tv.parse",
        "sk.parseOffsets",
        "mh.massHaul",
        "why.curve",
        "clip.read",
        "pe.check",
        "drive.sun",
        "project.fromJSON",
        "s3.inputRaw",
        "agent.run",
    ];
    let vals = [
        json!(null),
        json!(-1e308),
        json!("$NaN"),
        json!("x"),
        json!([]),
        json!({}),
        json!([{ "x": "$Inf", "y": 1 }]),
        json!({ "pts": [1, 2], "legs": "z" }),
        json!([[1, 2], [3]]),
    ];
    let mut s: u64 = 7;
    let mut next = || {
        s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (s >> 33) as usize
    };
    for _ in 0..400 {
        let op = ops[next() % ops.len()];
        let args: Vec<Value> = (0..(next() % 4)).map(|_| vals[next() % vals.len()].clone()).collect();
        let _ = cc_civil::script::run(
            &json!({ "calls": [{ "op": "tin.buildSurface", "args": [{ "points": [{ "x": 0, "y": 0, "z": 1 }, { "x": 1, "y": 0, "z": 1 }, { "x": 0, "y": 1, "z": 1 }] }], "as": "S" }, { "op": op, "args": args }] }),
        );
    }
}
