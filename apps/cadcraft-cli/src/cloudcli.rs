//! `cadcraft-cli cmds`: the point cloud under the drawing (cloud-cad.js, M4.50) in the parity scripts — the stub
//! host's cloud and picker, the inputs that open, snap to and draw it, and its state in every record, as
//! kdiff-cadcloud.mjs's stub has them in the JS.
//!
//! Script: `files.clouds: [{ name, path | b64 } | null]` (what `host.pickCloud()` hands POINTCLOUDATTACH, in order).
//! Inputs: `openCloud: { name, path | b64 }` (the window's Open: cadcraft.js `openLocal`), `cloudSnap: { x, y, r }`
//! (view.js `snapExtra`), `cloudDraw: { t: [a b c d e f], w, h, dpr }` (the underlay's picture, hashed).

use std::collections::VecDeque;

use cc_engine::PickedFile;
use cc_engine::cloud::{CloudAttach, Raster};
use cc_geom::P2;
use serde_json::{Value, json};

/// FNV-1a (64-bit) as hex (kdiff-cadcloud.mjs hashes the JS arrays' bytes the same way).
pub fn fnv64(b: impl IntoIterator<Item = u8>) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for x in b {
        h ^= u64::from(x);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// A file given as `{ name, path | b64 }`.
pub fn file_of(v: &Value, b64: &dyn Fn(&str) -> Option<Vec<u8>>) -> Option<PickedFile> {
    let name = v.get("name")?.as_str()?.to_string();
    let bytes = match (v.get("b64").and_then(Value::as_str), v.get("path").and_then(Value::as_str)) {
        (Some(b), _) => b64(b)?,
        (None, Some(p)) => std::fs::read(p).ok()?,
        _ => return None,
    };
    Some(PickedFile { name, bytes })
}

/// `files.clouds`
pub fn clouds_of(script: &Value, b64: &dyn Fn(&str) -> Option<Vec<u8>>) -> VecDeque<Option<PickedFile>> {
    script
        .get("files")
        .and_then(|f| f.get("clouds"))
        .and_then(Value::as_array)
        .map(|a| a.iter().map(|v| file_of(v, b64)).collect())
        .unwrap_or_default()
}

fn n(x: f64) -> Value {
    cc_cad::entity::num(x)
}

/// The cloud's state for a record (kdiff-cadcloud.mjs `cloudState`), None when no cloud is attached.
pub fn state(pc: &mut CloudAttach, units: &str) -> Option<Value> {
    let c = pc.cloud.clone()?;
    let shown = pc.shown(units).map(|s| fnv64(s.iter().flat_map(|v| v.to_le_bytes())));
    let classes: Vec<Value> = c.class_counts.iter().enumerate().filter(|(_, k)| **k > 0.0).map(|(cl, k)| json!([cl, n(*k)])).collect();
    let (h, t) = pc.slice_spec(units);
    Some(json!({
        "name": c.name, "kind": c.kind.name(), "n": c.n, "read": n(c.read),
        "origin": c.origin.iter().map(|v| n(*v)).collect::<Vec<_>>(),
        "min": c.min.iter().map(|v| n(*v)).collect::<Vec<_>>(), "max": c.max.iter().map(|v| n(*v)).collect::<Vec<_>>(),
        "unit": c.unit, "zUnit": c.z_unit, "notes": c.notes,
        "pos": fnv64(c.pos.iter().flat_map(|v| v.to_le_bytes())),
        "rgb": c.rgb.as_ref().map(|v| fnv64(v.iter().copied())),
        "inten": c.inten.as_ref().map(|v| fnv64(v.iter().flat_map(|x| x.to_le_bytes()))),
        "cls": c.cls.as_ref().map(|v| fnv64(v.iter().copied())),
        "classCounts": classes,
        "color": pc.color, "size": n(pc.size), "visible": pc.visible,
        "slice": { "on": pc.slice.on, "h": pc.slice.h.map(n), "thick": pc.slice.thick.map(n) },
        "spec": [n(h), n(t)],
        "insert": pc.insert.map(|p| json!({ "x": n(p.x), "y": n(p.y) })),
        "version": pc.version,
        "colors": pc.colors.as_ref().map(|v| fnv64(v.iter().copied())),
        "shown": shown,
        "bbox": pc.bbox(units).map(|b| b.iter().map(|v| n(*v)).collect::<Vec<_>>()),
    }))
}

/// `cloudSnap` / `cloudDraw` inputs → their event.
pub fn input(pc: &mut CloudAttach, inp: &Value, units: &str) -> Option<Value> {
    if let Some(s) = inp.get("cloudSnap") {
        let g = |k: &str| s.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
        let r = pc.snap(P2::new(g("x"), g("y")), g("r"), units);
        return Some(json!(["cloudSnap", r.map(|p| json!({ "x": n(p.x), "y": n(p.y) }))]));
    }
    if let Some(d) = inp.get("cloudDraw") {
        let g = |k: &str| d.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
        let mut t = [0.0; 6];
        if let Some(a) = d.get("t").and_then(Value::as_array) {
            for (s, v) in t.iter_mut().zip(a) {
                *s = v.as_f64().unwrap_or(f64::NAN);
            }
        }
        let r = pc.draw(t, g("w"), g("h"), g("dpr"), units);
        return Some(match r {
            Some(r) => json!(["cloudDraw", r.w, r.h, fnv64(r.rgba())]),
            None => json!(["cloudDraw", null]),
        });
    }
    None
}

/// `cadcraft-cli cloudbench <file> [units] [budget]`: open a cloud as POINTCLOUDATTACH does (read, attach), draw it
/// fitted in a 1600 × 1000 pane (every point, then the slice), snap 1000 times near its points; the results hashed
/// and each step timed (pcbench-cadcloud.mjs does the same in the frozen JS).
pub fn bench(rest: &[&str]) -> std::process::ExitCode {
    let Some(path) = rest.first() else {
        eprintln!("usage: cadcraft-cli cloudbench <file> [units] [budget]");
        return std::process::ExitCode::from(2);
    };
    let units = rest.get(1).copied().unwrap_or("meters");
    let budget = rest.get(2).and_then(|b| b.parse::<f64>().ok()).unwrap_or(2e6);
    let ms = |t: std::time::Instant| t.elapsed().as_secs_f64() * 1000.0;
    let t = std::time::Instant::now();
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            println!("{}", json!({ "error": e.to_string() }));
            return std::process::ExitCode::from(1);
        }
    };
    let read_ms = ms(t);
    let name = path.rsplit('/').next().unwrap_or(path).to_string();
    let tr = cc_cad::i18n::Catalog::new(
        &serde_json::from_str::<Value>(include_str!("../../../assets/lang/cadcraft/en.json")).unwrap_or(Value::Null),
    );
    let f = cc_io_civil::raster::InFile { name, bytes };
    // the coordinate system catalog, once per process (the page fetches it once): not part of the open
    let t = std::time::Instant::now();
    cc_io_civil::crs::set_catalog();
    let catalog_ms = ms(t);
    let t = std::time::Instant::now();
    let loaded =
        cc_io_civil::pointcloud::read_cloud_info(&tr, &f).and_then(|info| cc_io_civil::pointcloud::load_cloud(&tr, &f, &info, budget));
    let load_ms = ms(t);
    let cloud = match loaded {
        Ok(c) => c,
        Err(e) => {
            println!("{}", json!({ "error": e }));
            return std::process::ExitCode::SUCCESS;
        }
    };
    drop(f);
    let mut pc = CloudAttach::default();
    let t = std::time::Instant::now();
    pc.attach(cloud, None);
    let attach_ms = ms(t);
    let b = pc.bbox(units).unwrap_or([0.0, 0.0, 1.0, 1.0]);
    let (w, h) = (1600.0, 1000.0);
    let k = (w / (b[2] - b[0]).max(1e-9)).min(h / (b[3] - b[1]).max(1e-9)) * 0.9;
    let tt = [k, 0.0, 0.0, -k, w / 2.0 - k * (b[0] + b[2]) / 2.0, h / 2.0 + k * (b[1] + b[3]) / 2.0];
    let on = pc.slice.on;
    let hash = |r: Option<&Raster>| r.map(|r| fnv64(r.rgba()));
    pc.slice.on = false;
    pc.changed();
    let t = std::time::Instant::now();
    let drawn = pc.draw(tt, w, h, 1.0, units).is_some();
    let draw_ms = ms(t);
    let all = if drawn { hash(pc.draw(tt, w, h, 1.0, units)) } else { None };
    let t = std::time::Instant::now();
    let drawn = pc.draw(tt, w, h, 1.0, units).is_some();
    let redraw_ms = ms(t);
    let again = if drawn { hash(pc.draw(tt, w, h, 1.0, units)) } else { None };
    pc.slice.on = true;
    pc.changed();
    let t = std::time::Instant::now();
    let drawn = pc.draw(tt, w, h, 1.0, units).is_some();
    let slice_ms = ms(t);
    let sliced = if drawn { hash(pc.draw(tt, w, h, 1.0, units)) } else { None };
    pc.slice.on = on;
    pc.slice.on = false;
    pc.changed();
    let count = pc.cloud.as_ref().map_or(0, |c| c.n);
    let step = (count / 1000).max(1);
    let r = 2.0 / k;
    let t = std::time::Instant::now();
    let first = pc.snap(P2::new(b[0], b[1]), r, units);
    let grid_ms = ms(t);
    let t = std::time::Instant::now();
    let mut snaps: Vec<Value> = vec![first.map(|p| json!([n(p.x), n(p.y)])).unwrap_or(Value::Null)];
    for i in (0..count).step_by(step).take(1000) {
        let q = pc.drawing_point(i, units);
        let s = pc.snap(P2::new(q[0] + 0.3 * r, q[1] - 0.2 * r), r, units);
        snaps.push(s.map(|p| json!([n(p.x), n(p.y)])).unwrap_or(Value::Null));
    }
    let snap_ms = ms(t);
    let st = state(&mut pc, units);
    println!(
        "{}",
        json!({
            "state": st, "draw": all, "redraw": again == all, "slice": sliced,
            "snaps": fnv64(Value::Array(snaps).to_string().into_bytes()),
            "ms": { "catalog": catalog_ms, "read": read_ms, "load": load_ms, "attach": attach_ms, "draw": draw_ms, "redraw": redraw_ms, "slice": slice_ms, "grid": grid_ms, "snap1000": snap_ms },
        })
    );
    std::process::ExitCode::SUCCESS
}
