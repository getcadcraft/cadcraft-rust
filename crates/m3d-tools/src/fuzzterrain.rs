//! A byte-driven session for fuzzing terrain (fuzz/fuzz_targets/m3d_terrain.rs) and its regression tests: the bytes
//! choose terrain grids (any width, depth and spacing, hostile ones included), sculpt their corners, move the
//! definition's id counter near or past the kernel's dense id range, add and delete faces, then run what reads and
//! repairs the result (the solid checker's problems, info and Fix, and a re-face of the ground plane). Nothing may
//! panic, hang or run out of memory (M6.36: Fix on a big grid did).

use cc_m3d::Model;
use cc_m3d::ops::{add_polygon, rect_pts};
use cc_m3d::solids::{fix_solid, solid_info, solid_problems};
use serde_json::{Value, json};

/// Most operations one input runs.
pub const MAX_OPS: usize = 32;
/// Grids with more cells than this are not made here (the solid checker's Fix is quadratic in a plane's faces, as in
/// 0.5; a 200 × 200 grid takes seconds, which a fuzzer reads as a hang). M6.36's regression test covers the big grid.
pub const MAX_CELLS: f64 = 900.0;
/// The session stops when the model holds more faces than this.
pub const MAX_FACES: usize = 6_000;

const LENGTHS: [f64; 12] = [0.0, -1.0, 1.0, 2.5, 3.048, 20.0, 1e-300, 1e300, f64::NAN, f64::INFINITY, -0.0, 0.05];
const IDS: [f64; 7] = [1_040_000.0, 1_048_000.0, 1_048_575.0, 1_048_576.0, 4_294_967_296.0, 9_007_199_254_740_000.0, 1.0];

struct Bytes<'a> {
    d: &'a [u8],
    i: usize,
}
impl Bytes<'_> {
    fn u8(&mut self) -> Option<u8> {
        let b = self.d.get(self.i).copied();
        self.i = self.i.saturating_add(1);
        b
    }
    fn len(&mut self) -> Option<f64> {
        let k = self.u8()?;
        Some(if k < 128 { LENGTHS.get(usize::from(k) % LENGTHS.len()).copied().unwrap_or(1.0) } else { f64::from(k - 127) / 8.0 })
    }
}

/// The cells terrain_grid makes for these sizes (its own rule: round(len / spacing), 1 to 200).
fn cells(w: f64, d: f64, sp: f64) -> f64 {
    let c = |len: f64| {
        let n = (len / sp).round().clamp(1.0, 200.0);
        if n.is_finite() { n } else { 1.0 }
    };
    c(w) * c(d)
}

/// Run one input.
pub fn run(data: &[u8]) {
    let mut m = Model::new();
    let mut b = Bytes { d: data, i: 0 };
    let mut grids: Vec<String> = Vec::new();
    for _ in 0..MAX_OPS {
        let Some(op) = b.u8() else { break };
        let _ = step(&mut m, &mut b, op, &mut grids);
        let faces: usize = m.defs.values().map(|d| d.faces.len()).sum();
        if faces > MAX_FACES {
            break;
        }
    }
}

fn step(m: &mut Model, b: &mut Bytes<'_>, op: u8, grids: &mut Vec<String>) -> cc_m3d::Res<()> {
    if op.is_multiple_of(8) || grids.is_empty() {
        let (w, d, sp) = (b.len().unwrap_or(1.0), b.len().unwrap_or(1.0), b.len().unwrap_or(1.0));
        if cells(w, d, sp) > MAX_CELLS {
            return Ok(());
        }
        let id = m.change("terrain", |mm| Ok(crate::terrain::terrain_grid(mm, w, d, sp, [0.0; 3], "Terrain")))?;
        grids.push(id);
        return Ok(());
    }
    let k = usize::from(b.u8().unwrap_or(0));
    let Some(gid) = grids.get(k % grids.len()).cloned() else { return Ok(()) };
    let arg = b.u8().unwrap_or(0);
    if op % 8 == 6 && arg & 1 == 1 {
        renumber(m, IDS.get(usize::from(arg / 2) % IDS.len()).copied().unwrap_or(1.0));
        return Ok(());
    }
    m.change("edit", |mm| {
        let Some(d) = mm.def_mut(&gid) else { return Ok(()) };
        match op % 8 {
            1 => {
                // sculpt a corner up or down
                let n = d.verts.len();
                let v = if n > 0 { d.verts.keys().nth(usize::from(arg) % n) } else { None };
                if let Some(p) = v.and_then(|v| d.verts.get_mut(v)) {
                    p[2] += f64::from(arg % 7) - 3.0;
                }
            }
            2 => {
                let _ = fix_solid(d);
            }
            3 => {
                let _ = solid_problems(d);
                let _ = solid_info(d);
            }
            4 => {
                let all = cc_m3d::omap::IdSet::from_iter_ids(d.edges.keys());
                let _ = cc_m3d::edges::reface(d, &cc_m3d::vec::plane_from([0.0, 0.0, 1.0], [0.0; 3]), &all, None);
            }
            5 => {
                let f = d.faces.keys().nth(usize::from(arg));
                if let Some(f) = f {
                    d.faces.delete(f);
                }
            }
            6 => {
                // the id counter near the end of the dense range, past it, or near 2^53
                d.next = IDS.get(usize::from(arg) % IDS.len()).copied().unwrap_or(1.0);
            }
            _ => {
                let s = f64::from(arg % 16) / 4.0;
                add_polygon(d, &rect_pts([s, s / 2.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0.5, 0.75), None)?;
            }
        }
        Ok(())
    })
}

/// Every vertex, edge and face id of the model moved up by `by` (a save, renumbered, loaded again): the ids a model
/// that has been edited for a long time holds. A file that no longer loads leaves the model as it was.
fn renumber(m: &mut Model, by: f64) {
    let Ok(mut v) = serde_json::from_str::<Value>(&m.to_json_string(false)) else { return };
    let up = |x: &mut Value| {
        if let Some(n) = x.as_f64() {
            *x = json!(n + by);
        }
    };
    for d in v.get_mut("defs").and_then(Value::as_array_mut).into_iter().flatten() {
        if let Some(n) = d.get_mut("next") {
            up(n);
        }
        for r in d.get_mut("verts").and_then(Value::as_array_mut).into_iter().flatten() {
            if let Some(x) = r.get_mut(0) {
                up(x);
            }
        }
        for r in d.get_mut("edges").and_then(Value::as_array_mut).into_iter().flatten() {
            for i in 0..3 {
                if let Some(x) = r.get_mut(i) {
                    up(x);
                }
            }
        }
        for f in d.get_mut("faces").and_then(Value::as_array_mut).into_iter().flatten() {
            if let Some(x) = f.get_mut("id") {
                up(x);
            }
            for l in f.get_mut("loops").and_then(Value::as_array_mut).into_iter().flatten() {
                for x in l.as_array_mut().into_iter().flatten() {
                    up(x);
                }
            }
        }
    }
    let mut m2 = Model::new();
    if m2.load(&v, false).is_ok() {
        *m = m2;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn some_inputs() {
        super::run(&[]);
        let all: Vec<u8> = (0..=255).collect();
        super::run(&all);
        let rev: Vec<u8> = (0..=255).rev().collect();
        super::run(&rev);
    }

    /// M6.36's seed: a 24 × 20 grid renumbered to the end of the kernel's dense id range, a rectangle drawn on it,
    /// then Fix and a re-face of the ground (one id set per cycle, each as long as the largest id: 4 GB before).
    #[test]
    fn m636_seed() {
        let t = std::time::Instant::now();
        super::run(include_bytes!("../../../fuzz/seeds/m3d_terrain/m636-fix-grid.bin"));
        assert!(t.elapsed().as_secs() < 60, "{:?}", t.elapsed());
    }
}
