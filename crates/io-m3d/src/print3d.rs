//! print3d.js — is a mesh ready for a 3D printer, and the files a slicer reads (0.5 SG.4). Printer meshes are in
//! millimetres, Z up (the bed is the XY plane), triangles wound counter-clockwise seen from outside.
//!
//!   mesh_from_triangles(tris) → Mesh · weld(mesh, tol) · print_check(mesh, opts) → Report · stl_binary(mesh, header)
//!   three_mf(mesh, name, app) (3MF core in a stored zip) · crc32 · zip_stored · tri_tri

// Index arithmetic here (`t * 3 + k`, `v * 3 + k`) stays below the lengths of the arrays it indexes (every index is
// read through `.get()`), and those lengths are bounded by memory: no product can overflow a usize.
#![allow(clippy::arithmetic_side_effects)]

use std::collections::HashSet;

use cc_m3d::js;
use cc_m3d::omap::FxMap;
use serde_json::{Map, Value, json};

use crate::jsx::cos;
use crate::num::{precision_num, to_i32};
use crate::stl::f32_of;

/// A printer mesh: corner positions (x, y, z per corner), triangles (three corner indices each), a colour per triangle.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<f64>,
    pub indices: Vec<u32>,
    pub colors: Option<Vec<Option<String>>>,
    pub degenerate: f64,
}

/// One input triangle: three corners and an optional colour.
#[derive(Clone, Debug, PartialEq)]
pub struct Tri {
    pub p: [[f64; 3]; 3],
    pub color: Option<String>,
}

/// `meshFromTriangles(tris)`.
pub fn mesh_from_triangles(tris: &[Tri]) -> Mesh {
    let n = tris.len();
    let mut positions = Vec::with_capacity(n.saturating_mul(9));
    let mut indices = Vec::with_capacity(n.saturating_mul(3));
    let mut colors: Option<Vec<Option<String>>> = None;
    for (i, t) in tris.iter().enumerate() {
        for k in 0..3usize {
            let p = t.p.get(k).copied().unwrap_or([f64::NAN; 3]);
            positions.extend_from_slice(&p);
            indices.push(u32::try_from(i.saturating_mul(3).saturating_add(k)).unwrap_or(u32::MAX));
        }
        if let Some(c) = t.color.as_ref().filter(|c| !c.is_empty()) {
            let cols = colors.get_or_insert_with(|| vec![None; n]);
            if let Some(slot) = cols.get_mut(i) {
                *slot = Some(c.clone());
            }
        }
    }
    Mesh { positions, indices, colors, degenerate: 0.0 }
}

#[inline]
fn tri_normal(p: &[f64], a: usize, b: usize, c: usize) -> [f64; 3] {
    let g = |i: usize, k: usize| p.get(i.saturating_mul(3).saturating_add(k)).copied().unwrap_or(f64::NAN);
    let (ux, uy, uz) = (g(b, 0) - g(a, 0), g(b, 1) - g(a, 1), g(b, 2) - g(a, 2));
    let (vx, vy, vz) = (g(c, 0) - g(a, 0), g(c, 1) - g(a, 1), g(c, 2) - g(a, 2));
    [uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx]
}

/// `hash(x, y, z)` of cell indices (each through ToInt32, as Math.imul takes them).
#[inline]
fn cell_hash(x: f64, y: f64, z: f64) -> i32 {
    to_i32(x).wrapping_mul(73_856_093) ^ to_i32(y).wrapping_mul(19_349_663) ^ to_i32(z).wrapping_mul(83_492_791)
}

/// `weld(mesh, tol)`: corners within tol merged; triangles that collapse dropped and counted.
pub fn weld(mesh: &Mesh, tol: f64) -> Mesh {
    let pp = &mesh.positions;
    let n_in = pp.len() / 3;
    let cs = tol * 4.0;
    let mut cell: FxMap<i32, Vec<usize>> = FxMap::default();
    let mut remap: Vec<usize> = Vec::with_capacity(n_in);
    let mut out: Vec<f64> = Vec::with_capacity(pp.len());
    let r = 0.25 + 1e-9;
    for i in 0..n_in {
        let g = |k: usize| pp.get(i.saturating_mul(3).saturating_add(k)).copied().unwrap_or(f64::NAN);
        let (x, y, z) = (g(0), g(1), g(2));
        let (fx, fy, fz) = (x / cs, y / cs, z / cs);
        let (cx, cy, cz) = (fx.floor(), fy.floor(), fz.floor());
        let lohi = |f: f64, c: f64| -> (f64, f64) { (if f - c < r { -1.0 } else { 0.0 }, if c + 1.0 - f < r { 1.0 } else { 0.0 }) };
        let ((x0, x1), (y0, y1), (z0, z1)) = (lohi(fx, cx), lohi(fy, cy), lohi(fz, cz));
        let mut found: Option<usize> = None;
        let mut dx = x0;
        'outer: while dx <= x1 {
            let mut dy = y0;
            while dy <= y1 {
                let mut dz = z0;
                while dz <= z1 {
                    if let Some(list) = cell.get(&cell_hash(cx + dx, cy + dy, cz + dz)) {
                        for &j in list {
                            let o = |k: usize| out.get(j.saturating_mul(3).saturating_add(k)).copied().unwrap_or(f64::NAN);
                            if (o(0) - x).abs() <= tol && (o(1) - y).abs() <= tol && (o(2) - z).abs() <= tol {
                                found = Some(j);
                                break 'outer;
                            }
                        }
                    }
                    dz += 1.0;
                }
                dy += 1.0;
            }
            dx += 1.0;
        }
        let f = match found {
            Some(j) => j,
            None => {
                let j = out.len() / 3;
                out.extend_from_slice(&[x, y, z]);
                cell.entry(cell_hash(cx, cy, cz)).or_default().push(j);
                j
            }
        };
        remap.push(f);
    }
    let mut idx: Vec<u32> = Vec::with_capacity(mesh.indices.len());
    let mut cols: Option<Vec<Option<String>>> = mesh.colors.as_ref().map(|_| Vec::new());
    let mut degenerate = 0.0;
    let area2 = tol * tol * 1e-2;
    let rm = |k: usize| -> usize {
        mesh.indices.get(k).and_then(|&i| usize::try_from(i).ok()).and_then(|i| remap.get(i)).copied().unwrap_or(usize::MAX)
    };
    for t in 0..mesh.indices.len() / 3 {
        let (a, b, c) = (rm(t * 3), rm(t * 3 + 1), rm(t * 3 + 2));
        if a == b || b == c || a == c {
            degenerate += 1.0;
            continue;
        }
        let n = tri_normal(&out, a, b, c);
        if n[0] * n[0] + n[1] * n[1] + n[2] * n[2] <= area2 * area2 {
            degenerate += 1.0;
            continue;
        }
        for v in [a, b, c] {
            idx.push(u32::try_from(v).unwrap_or(u32::MAX));
        }
        if let (Some(cl), Some(src)) = (cols.as_mut(), mesh.colors.as_ref()) {
            cl.push(src.get(t).cloned().flatten().filter(|s| !s.is_empty()));
        }
    }
    let colors = cols.filter(|c| c.iter().any(Option::is_some));
    Mesh { positions: out, indices: idx, colors, degenerate }
}

/// `printCheck` options.
#[derive(Clone, Debug)]
pub struct CheckOpts {
    pub tol: f64,
    pub bed: Option<[f64; 3]>,
    pub overhang: f64,
    pub max_tests: f64,
}
impl Default for CheckOpts {
    fn default() -> Self {
        Self { tol: 1e-4, bed: None, overhang: 45.0, max_tests: 4e6 }
    }
}

/// An insertion-ordered map with u64 keys (a JS Map of numbers).
struct OMapU<V> {
    idx: FxMap<u64, usize>,
    items: Vec<(u64, V)>,
}
impl<V> OMapU<V> {
    fn new() -> Self {
        Self { idx: FxMap::default(), items: Vec::new() }
    }
    fn get_or_insert(&mut self, k: u64, f: impl FnOnce() -> V) -> &mut V {
        let i = match self.idx.get(&k) {
            Some(&i) => i,
            None => {
                self.items.push((k, f()));
                let i = self.items.len().saturating_sub(1);
                self.idx.insert(k, i);
                i
            }
        };
        // i is an index of items (just pushed or recorded)
        #[allow(clippy::indexing_slicing)]
        &mut self.items[i].1
    }
}

/// An edge's record: [uses a→b, uses b→a], first triangle, a, b, the triangles on it.
type EdgeUse = ([u32; 2], usize, usize, usize, Vec<usize>);

/// The printer report (`printCheck`), with the welded mesh it checked.
#[derive(Clone, Debug)]
pub struct Report {
    pub value: Map<String, Value>,
    pub mesh: Mesh,
    pub ok: bool,
}

fn pts_value(v: &[[f64; 3]]) -> Value {
    Value::Array(v.iter().map(|p| json!([cc_m3d::file::jn(p[0]), cc_m3d::file::jn(p[1]), cc_m3d::file::jn(p[2])])).collect())
}

/// `printCheck(mesh, opts)`.
#[allow(clippy::too_many_lines)]
pub fn print_check(mesh_in: &Mesh, opts: &CheckOpts) -> Report {
    let mesh = weld(mesh_in, opts.tol);
    let p = &mesh.positions;
    let ii = &mesh.indices;
    let nt = ii.len() / 3;
    let nv = p.len() / 3;
    let pt = |v: usize| -> [f64; 3] {
        let g = |k: usize| p.get(v.saturating_mul(3).saturating_add(k)).copied().unwrap_or(f64::NAN);
        [g(0), g(1), g(2)]
    };
    let iv = |k: usize| -> usize { ii.get(k).and_then(|&x| usize::try_from(x).ok()).unwrap_or(0) };
    let jn = cc_m3d::file::jn;
    let mut rep = Map::new();
    let put = |rep: &mut Map<String, Value>, k: &str, v: Value| {
        rep.insert(k.to_string(), v);
    };
    put(&mut rep, "ok", Value::Bool(false));
    put(&mut rep, "triangles", json!(nt));
    put(&mut rep, "vertices", json!(nv));
    let mut problems: Vec<Value> = Vec::new();
    let mut warnings: Vec<Value> = Vec::new();
    if nt == 0 {
        problems.push(json!({ "kind": "empty", "n": 0, "at": [] }));
        let keys = [
            ("shells", json!(0)),
            ("volume", Value::Null),
            ("area", json!(0)),
            ("min", Value::Null),
            ("max", Value::Null),
            ("size", Value::Null),
            ("open", json!(0)),
            ("nonManifold", json!(0)),
            ("touching", json!(0)),
            ("flipped", json!(0)),
            ("inverted", json!(0)),
            ("selfIntersections", json!(0)),
            ("overlaps", json!(0)),
            ("degenerate", jn(mesh.degenerate)),
            ("checked", Value::Bool(true)),
            ("overhangArea", json!(0)),
            ("bedArea", json!(0)),
            ("problems", Value::Array(problems)),
            ("warnings", Value::Array(warnings)),
        ];
        for (k, v) in keys {
            put(&mut rep, k, v);
        }
        return Report { value: rep, mesh, ok: false };
    }
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for v in 0..nv {
        let q = pt(v);
        for ((l, h), x) in lo.iter_mut().zip(hi.iter_mut()).zip(q) {
            if x < *l {
                *l = x;
            }
            if x > *h {
                *h = x;
            }
        }
    }
    let size = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
    // edges: key a*nv+b (a < b) → [uses a→b, uses b→a, first triangle, a, b], and the triangles on each
    let nvu = nv as u64;
    let ek = |a: usize, b: usize| -> u64 {
        let (x, y) = if a < b { (a as u64, b as u64) } else { (b as u64, a as u64) };
        x.saturating_mul(nvu).saturating_add(y)
    };
    let mut edges: OMapU<EdgeUse> = OMapU::new();
    for t in 0..nt {
        for k in 0..3usize {
            let a = iv(t * 3 + k);
            let b = iv(t * 3 + (k + 1) % 3);
            let e = edges.get_or_insert(ek(a, b), || ([0, 0], t, a, b, Vec::new()));
            let slot = if a < b { &mut e.0[0] } else { &mut e.0[1] };
            *slot = slot.saturating_add(1);
            e.4.push(t);
        }
    }
    // shells: triangles joined through their edges
    let mut parent: Vec<usize> = (0..nt).collect();
    fn find(parent: &mut [usize], mut x: usize) -> usize {
        loop {
            let px = parent.get(x).copied().unwrap_or(x);
            if px == x {
                return x;
            }
            let ppx = parent.get(px).copied().unwrap_or(px);
            if let Some(s) = parent.get_mut(x) {
                *s = ppx;
            }
            x = ppx;
        }
    }
    let mid = |a: usize, b: usize| -> [f64; 3] {
        let (pa, pb) = (pt(a), pt(b));
        [(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0, (pa[2] + pb[2]) / 2.0]
    };
    let (mut open, mut touching, mut non_manifold, mut flipped) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let mut at_open: Vec<[f64; 3]> = Vec::new();
    let mut at_touch: Vec<[f64; 3]> = Vec::new();
    let mut at_nm: Vec<[f64; 3]> = Vec::new();
    let mut at_flip: Vec<[f64; 3]> = Vec::new();
    let mut bad_shell: Vec<usize> = Vec::new();
    for (_, (uses, _t, a, b, ts)) in &edges.items {
        let n = uses[0].saturating_add(uses[1]);
        let t0 = ts.first().copied().unwrap_or(0);
        for &ti in ts.iter().skip(1) {
            let ra = find(&mut parent, t0);
            let rb = find(&mut parent, ti);
            if ra != rb
                && let Some(s) = parent.get_mut(ra)
            {
                *s = rb;
            }
        }
        if n == 1 {
            open += 1.0;
            if at_open.len() < 20 {
                at_open.push(mid(*a, *b));
            }
            bad_shell.push(t0);
        } else if n > 2 && uses[0] == uses[1] {
            touching += 1.0;
            if at_touch.len() < 20 {
                at_touch.push(mid(*a, *b));
            }
        } else if n > 2 {
            non_manifold += 1.0;
            if at_nm.len() < 20 {
                at_nm.push(mid(*a, *b));
            }
            bad_shell.push(t0);
        } else if uses[0] == 2 || uses[1] == 2 {
            flipped += 1.0;
            if at_flip.len() < 20 {
                at_flip.push(mid(*a, *b));
            }
            bad_shell.push(t0);
        }
    }
    let mut shell_of: Vec<usize> = vec![0; nt];
    let mut roots: FxMap<usize, usize> = FxMap::default();
    for t in 0..nt {
        let r = find(&mut parent, t);
        let n = roots.len();
        let s = *roots.entry(r).or_insert(n);
        if let Some(x) = shell_of.get_mut(t) {
            *x = s;
        }
    }
    let shells = roots.len();
    let broken: HashSet<usize> = bad_shell.iter().map(|&t| shell_of.get(t).copied().unwrap_or(0)).collect();
    // volume and area per shell; overhangs; bed contact
    let mut vol = vec![0.0f64; shells];
    let cos_limit = cos((90.0 - opts.overhang) * std::f64::consts::PI / 180.0);
    let mut zmax = 1.0f64;
    for s in size {
        zmax = js::max(zmax, s);
    }
    let z_tol = js::max(opts.tol * 10.0, 1e-6 * zmax);
    let (mut area, mut bed_area, mut overhang_area) = (0.0f64, 0.0f64, 0.0f64);
    let g = |v: usize, k: usize| p.get(v.saturating_mul(3).saturating_add(k)).copied().unwrap_or(f64::NAN);
    for t in 0..nt {
        let (a, b, c) = (iv(t * 3), iv(t * 3 + 1), iv(t * 3 + 2));
        let n = tri_normal(p, a, b, c);
        let l = js::hypot(&n);
        let ar = l / 2.0;
        area += ar;
        if let Some(v) = vol.get_mut(shell_of.get(t).copied().unwrap_or(0)) {
            *v += (g(a, 0) * (g(b, 1) * g(c, 2) - g(b, 2) * g(c, 1)) - g(a, 1) * (g(b, 0) * g(c, 2) - g(b, 2) * g(c, 0))
                + g(a, 2) * (g(b, 0) * g(c, 1) - g(b, 1) * g(c, 0)))
                / 6.0;
        }
        let nz = if l != 0.0 && !l.is_nan() { n[2] / l } else { 0.0 };
        let on_bed = (g(a, 2) - lo[2]).abs() <= z_tol && (g(b, 2) - lo[2]).abs() <= z_tol && (g(c, 2) - lo[2]).abs() <= z_tol;
        if on_bed && nz < -0.999 {
            bed_area += ar;
        } else if nz < -cos_limit {
            overhang_area += ar;
        }
    }
    // a closed shell with negative volume is inside out, unless it is a hollow inside another shell
    let mut inverted = 0.0f64;
    let mut inv: Vec<Value> = Vec::new();
    let mut tested = 0u64;
    for s in 0..shells {
        if broken.contains(&s) || vol.get(s).copied().unwrap_or(0.0) >= 0.0 {
            continue;
        }
        let sp = (0..nt).find(|&t| shell_of.get(t) == Some(&s)).map(|t| pt(iv(t * 3)));
        let pre = tested;
        tested = tested.saturating_add(1);
        if pre < 200
            && let Some(q) = sp
        {
            // a ray from q, a little off every axis, crossing the other shells an odd number of times
            let d = [0.000123, 0.000457, 1.0];
            let mut hits = 0usize;
            for t in 0..nt {
                if shell_of.get(t) == Some(&s) {
                    continue;
                }
                let h = ray_hit(q, d, pt(iv(t * 3)), pt(iv(t * 3 + 1)), pt(iv(t * 3 + 2)));
                if h > 1e-9 {
                    hits = hits.saturating_add(1);
                }
            }
            if hits % 2 == 1 {
                continue;
            }
        }
        inverted += 1.0;
        if inv.len() < 20 {
            inv.push(match sp {
                Some(q) => pts_value(&[q]).as_array().and_then(|a| a.first().cloned()).unwrap_or(Value::Null),
                None => Value::Null,
            });
        }
    }
    let si = self_intersections(p, ii, nt, &shell_of, size, opts.max_tests);
    if open > 0.0 {
        problems.push(json!({ "kind": "open", "n": open, "at": pts_value(&at_open) }));
    }
    if non_manifold > 0.0 {
        problems.push(json!({ "kind": "nonmanifold", "n": non_manifold, "at": pts_value(&at_nm) }));
    }
    if flipped > 0.0 {
        problems.push(json!({ "kind": "flipped", "n": flipped, "at": pts_value(&at_flip) }));
    }
    if inverted > 0.0 {
        problems.push(json!({ "kind": "inverted", "n": inverted, "at": Value::Array(inv) }));
    }
    if si.same > 0.0 {
        problems.push(json!({ "kind": "selfintersect", "n": si.same, "at": pts_value(&si.at_same) }));
    }
    let ok = problems.is_empty();
    let volume = if ok { jn(vol.iter().sum::<f64>()) } else { Value::Null };
    let volume = if ok {
        let mut v = 0.0;
        for x in &vol {
            v += x;
        }
        jn(v)
    } else {
        let _ = volume;
        Value::Null
    };
    if si.other > 0.0 {
        warnings.push(json!({ "kind": "overlap", "n": si.other, "at": pts_value(&si.at_other) }));
    }
    if touching > 0.0 {
        warnings.push(json!({ "kind": "touch", "n": touching, "at": pts_value(&at_touch) }));
    }
    let mut smax = f64::NEG_INFINITY;
    for s in size {
        smax = js::max(smax, s);
    }
    if smax < 1.0 {
        warnings.push(json!({ "kind": "tiny", "size": pts_value(&[size]).as_array().and_then(|a| a.first().cloned()) }));
    }
    if let Some(bed) = opts.bed {
        let fit = |s: [f64; 3], b: [f64; 3]| s[0] <= b[0] + 1e-9 && s[1] <= b[1] + 1e-9 && s[2] <= b[2] + 1e-9;
        if !fit(size, bed) && !fit([size[1], size[0], size[2]], bed) {
            warnings.push(
                json!({ "kind": "bed", "size": [jn(size[0]), jn(size[1]), jn(size[2])], "bed": [jn(bed[0]), jn(bed[1]), jn(bed[2])] }),
            );
        }
    }
    if bed_area <= 0.0 {
        warnings.push(json!({ "kind": "nobase" }));
    }
    if overhang_area > 0.01 * area {
        warnings
            .push(json!({ "kind": "overhang", "area": jn(overhang_area), "share": jn(overhang_area / area), "angle": jn(opts.overhang) }));
    }
    if mesh.degenerate > 0.0 {
        warnings.push(json!({ "kind": "degenerate", "n": mesh.degenerate }));
    }
    if !si.checked {
        warnings.push(json!({ "kind": "unchecked" }));
    }
    let keys = [
        ("shells", json!(shells)),
        ("volume", volume),
        ("area", jn(area)),
        ("min", json!([jn(lo[0]), jn(lo[1]), jn(lo[2])])),
        ("max", json!([jn(hi[0]), jn(hi[1]), jn(hi[2])])),
        ("size", json!([jn(size[0]), jn(size[1]), jn(size[2])])),
        ("open", jn(open)),
        ("nonManifold", jn(non_manifold)),
        ("touching", jn(touching)),
        ("flipped", jn(flipped)),
        ("inverted", jn(inverted)),
        ("selfIntersections", jn(si.same)),
        ("overlaps", jn(si.other)),
        ("degenerate", jn(mesh.degenerate)),
        ("checked", Value::Bool(si.checked)),
        ("overhangArea", jn(overhang_area)),
        ("bedArea", jn(bed_area)),
        ("problems", Value::Array(problems)),
        ("warnings", Value::Array(warnings)),
    ];
    put(&mut rep, "ok", Value::Bool(ok));
    for (k, v) in keys {
        put(&mut rep, k, v);
    }
    Report { value: rep, mesh, ok }
}

struct Si {
    same: f64,
    other: f64,
    at_same: Vec<[f64; 3]>,
    at_other: Vec<[f64; 3]>,
    checked: bool,
}

/// `selfIntersections(P, I, nt, shellOf, size, maxTests)`: a uniform grid over the triangles' boxes; pairs that share
/// a corner are neighbours, not tested.
#[allow(clippy::too_many_lines)]
fn self_intersections(p: &[f64], ii: &[u32], nt: usize, shell_of: &[usize], size: [f64; 3], max_tests: f64) -> Si {
    let mut res = Si { same: 0.0, other: 0.0, at_same: Vec::new(), at_other: Vec::new(), checked: true };
    let diag0 = js::hypot(&size);
    let diag = if diag0 != 0.0 && !diag0.is_nan() { diag0 } else { 1.0 };
    let eps = diag * 1e-9;
    let iv = |k: usize| -> usize { ii.get(k).and_then(|&x| usize::try_from(x).ok()).unwrap_or(0) };
    let pc = |v: usize, k: usize| p.get(v.saturating_mul(3).saturating_add(k)).copied().unwrap_or(f64::NAN);
    let mut bx = vec![0.0f64; nt.saturating_mul(6)];
    let mut ext = 0.0;
    for t in 0..nt {
        for k in 0..3usize {
            let (a, b, c) = (pc(iv(t * 3), k), pc(iv(t * 3 + 1), k), pc(iv(t * 3 + 2), k));
            let lo = js::min(js::min(a, b), c) - eps;
            let hi = js::max(js::max(a, b), c) + eps;
            if let Some(s) = bx.get_mut(t * 6 + k) {
                *s = lo;
            }
            if let Some(s) = bx.get_mut(t * 6 + 3 + k) {
                *s = hi;
            }
            ext += hi - lo;
        }
    }
    let bxg = |i: usize| bx.get(i).copied().unwrap_or(f64::NAN);
    // cell size about twice the mean triangle extent, no more than ~2M cells
    let mut cs = js::max((ext / (nt as f64 * 3.0)) * 2.0, diag / 1e3);
    let cells_of = |s: f64, cs: f64| js::max(1.0, (s / cs).ceil());
    let orc = |s: f64, cs: f64| if s != 0.0 && !s.is_nan() { s } else { cs };
    let mut guard = 0u32;
    while cells_of(orc(size[0], cs), cs) * cells_of(orc(size[1], cs), cs) * cells_of(orc(size[2], cs), cs) > 2e6 {
        cs *= 1.5;
        guard = guard.saturating_add(1);
        if guard > 10_000 {
            break;
        }
    }
    let mut o = [bxg(0), bxg(1), bxg(2)];
    for t in 0..nt {
        for (k, ok) in o.iter_mut().enumerate() {
            *ok = js::min(*ok, bxg(t * 6 + k));
        }
    }
    let nx = cells_of(size[0] + 4.0 * eps, cs);
    let ny = cells_of(size[1] + 4.0 * eps, cs);
    let nz = cells_of(size[2] + 4.0 * eps, cs);
    let nn = [nx, ny, nz];
    let ci = |v: f64, k: usize| {
        js::max(0.0, js::min(nn.get(k).copied().unwrap_or(1.0) - 1.0, ((v - o.get(k).copied().unwrap_or(0.0)) / cs).floor()))
    };
    let mut grid: OMapU<Vec<usize>> = OMapU::new();
    let mut big: Vec<usize> = Vec::new();
    let mut is_big = vec![false; nt];
    for t in 0..nt {
        let (x0, x1) = (ci(bxg(t * 6), 0), ci(bxg(t * 6 + 3), 0));
        let (y0, y1) = (ci(bxg(t * 6 + 1), 1), ci(bxg(t * 6 + 4), 1));
        let (z0, z1) = (ci(bxg(t * 6 + 2), 2), ci(bxg(t * 6 + 5), 2));
        if (x1 - x0 + 1.0) * (y1 - y0 + 1.0) * (z1 - z0 + 1.0) > 512.0 {
            big.push(t);
            if let Some(b) = is_big.get_mut(t) {
                *b = true;
            }
            continue;
        }
        let mut x = x0;
        while x <= x1 {
            let mut y = y0;
            while y <= y1 {
                let mut z = z0;
                while z <= z1 {
                    let k = (x * ny + y) * nz + z;
                    grid.get_or_insert(k.to_bits(), Vec::new).push(t);
                    z += 1.0;
                }
                y += 1.0;
            }
            x += 1.0;
        }
    }
    let mut tests = 0.0f64;
    let overlap = |a: usize, b: usize| {
        bxg(a * 6) <= bxg(b * 6 + 3)
            && bxg(b * 6) <= bxg(a * 6 + 3)
            && bxg(a * 6 + 1) <= bxg(b * 6 + 4)
            && bxg(b * 6 + 1) <= bxg(a * 6 + 4)
            && bxg(a * 6 + 2) <= bxg(b * 6 + 5)
            && bxg(b * 6 + 2) <= bxg(a * 6 + 5)
    };
    let shares = |a: usize, b: usize| (0..3).any(|i| (0..3).any(|j| ii.get(a * 3 + i) == ii.get(b * 3 + j)));
    let vtx = |t: usize, k: usize| -> [f64; 3] {
        let v = iv(t * 3 + k);
        [pc(v, 0), pc(v, 1), pc(v, 2)]
    };
    let mut test = |res: &mut Si, a: usize, b: usize| {
        if !overlap(a, b) || shares(a, b) {
            return;
        }
        tests += 1.0;
        if tests > max_tests {
            res.checked = false;
            return;
        }
        if !tri_tri(vtx(a, 0), vtx(a, 1), vtx(a, 2), vtx(b, 0), vtx(b, 1), vtx(b, 2), eps * 10.0) {
            return;
        }
        let q = vtx(a, 0);
        if shell_of.get(a) == shell_of.get(b) {
            res.same += 1.0;
            if res.at_same.len() < 20 {
                res.at_same.push(q);
            }
        } else {
            res.other += 1.0;
            if res.at_other.len() < 20 {
                res.at_other.push(q);
            }
        }
    };
    for (kb, l) in &grid.items {
        if !res.checked {
            break;
        }
        let k = f64::from_bits(*kb);
        let z = k % nz;
        let y = ((k - z) / nz) % ny;
        let x = (k - z - y * nz) / (nz * ny);
        let mut i = 0usize;
        while i < l.len() && res.checked {
            let mut j = i + 1;
            while j < l.len() {
                let (a, b) = (l.get(i).copied().unwrap_or(0), l.get(j).copied().unwrap_or(0));
                j += 1;
                // test each pair once: in the cell holding the low corner of the two boxes' overlap
                if ci(bxg(a * 6).max(bxg(b * 6)), 0) != x
                    || ci(bxg(a * 6 + 1).max(bxg(b * 6 + 1)), 1) != y
                    || ci(bxg(a * 6 + 2).max(bxg(b * 6 + 2)), 2) != z
                {
                    continue;
                }
                test(&mut res, a, b);
            }
            i += 1;
        }
    }
    for &a in &big {
        let mut b = 0usize;
        while b < nt && res.checked {
            if b != a && (!is_big.get(b).copied().unwrap_or(false) || b > a) {
                test(&mut res, a, b);
            }
            b += 1;
        }
    }
    res
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// `rayHit(o, d, a, b, c)`: Möller–Trumbore, the distance along d where the ray from o meets the triangle, or -1.
fn ray_hit(o: [f64; 3], d: [f64; 3], a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    let e1 = sub(b, a);
    let e2 = sub(c, a);
    let h = cross(d, e2);
    let det = dot(e1, h);
    if det.abs() < 1e-18 {
        return -1.0;
    }
    let f = 1.0 / det;
    let s = sub(o, a);
    let u = f * dot(s, h);
    if u < 0.0 || u > 1.0 {
        return -1.0;
    }
    let q = cross(s, e1);
    let v = f * dot(d, q);
    if v < 0.0 || u + v > 1.0 {
        return -1.0;
    }
    f * dot(e2, q)
}

/// `triTri(...)`: do two triangles cross (more than touch)? Möller's interval test; coplanar pairs in 2D.
#[allow(clippy::too_many_arguments)]
pub fn tri_tri(a0: [f64; 3], a1: [f64; 3], a2: [f64; 3], b0: [f64; 3], b1: [f64; 3], b2: [f64; 3], eps: f64) -> bool {
    let n_b = cross(sub(b1, b0), sub(b2, b0));
    let l_b = js::hypot(&n_b);
    let n_a = cross(sub(a1, a0), sub(a2, a0));
    let l_a = js::hypot(&n_a);
    if l_a == 0.0 || l_a.is_nan() || l_b == 0.0 || l_b.is_nan() {
        return false;
    }
    let u_b = [n_b[0] / l_b, n_b[1] / l_b, n_b[2] / l_b];
    let u_a = [n_a[0] / l_a, n_a[1] / l_a, n_a[2] / l_a];
    let snap = |d: f64| if d.abs() <= eps { 0.0 } else { d };
    let da = [snap(dot(u_b, sub(a0, b0))), snap(dot(u_b, sub(a1, b0))), snap(dot(u_b, sub(a2, b0)))];
    if (da[0] > 0.0 && da[1] > 0.0 && da[2] > 0.0) || (da[0] < 0.0 && da[1] < 0.0 && da[2] < 0.0) {
        return false;
    }
    let db = [snap(dot(u_a, sub(b0, a0))), snap(dot(u_a, sub(b1, a0))), snap(dot(u_a, sub(b2, a0)))];
    if (db[0] > 0.0 && db[1] > 0.0 && db[2] > 0.0) || (db[0] < 0.0 && db[1] < 0.0 && db[2] < 0.0) {
        return false;
    }
    if da[0] == 0.0 && da[1] == 0.0 && da[2] == 0.0 {
        return coplanar([a0, a1, a2], [b0, b1, b2], u_a, eps);
    }
    let d = cross(u_a, u_b);
    let ax = if d[0].abs() > d[1].abs() {
        if d[0].abs() > d[2].abs() { 0 } else { 2 }
    } else if d[1].abs() > d[2].abs() {
        1
    } else {
        2
    };
    let ivl = |pts: [[f64; 3]; 3], dd: [f64; 3]| -> Option<(f64, f64)> {
        let c = |p: [f64; 3]| p.get(ax).copied().unwrap_or(f64::NAN);
        let pr = [c(pts[0]), c(pts[1]), c(pts[2])];
        let mut xs: Vec<f64> = Vec::new();
        for (i, j) in [(0usize, 1usize), (1, 2), (2, 0)] {
            let (di, dj) = (dd.get(i).copied().unwrap_or(f64::NAN), dd.get(j).copied().unwrap_or(f64::NAN));
            let (pi, pj) = (pr.get(i).copied().unwrap_or(f64::NAN), pr.get(j).copied().unwrap_or(f64::NAN));
            if di == 0.0 {
                xs.push(pi);
            }
            if di * dj < 0.0 {
                xs.push(pi + (pj - pi) * di / (di - dj));
            }
        }
        if xs.is_empty() {
            return None;
        }
        let mut mn = f64::INFINITY;
        let mut mx = f64::NEG_INFINITY;
        for x in xs {
            mn = js::min(mn, x);
            mx = js::max(mx, x);
        }
        Some((mn, mx))
    };
    let (Some(a), Some(b)) = (ivl([a0, a1, a2], da), ivl([b0, b1, b2], db)) else { return false };
    js::max(a.0, b.0) < js::min(a.1, b.1) - eps
}

fn coplanar(a: [[f64; 3]; 3], b: [[f64; 3]; 3], n: [f64; 3], eps: f64) -> bool {
    let ax = if n[0].abs() > n[1].abs() {
        if n[0].abs() > n[2].abs() { 0 } else { 2 }
    } else if n[1].abs() > n[2].abs() {
        1
    } else {
        2
    };
    let (i, j) = match ax {
        0 => (1usize, 2usize),
        1 => (2, 0),
        _ => (0, 1),
    };
    let pick = |p: &[f64; 3]| [p.get(i).copied().unwrap_or(f64::NAN), p.get(j).copied().unwrap_or(f64::NAN)];
    let a2: Vec<[f64; 2]> = a.iter().map(pick).collect();
    let b2: Vec<[f64; 2]> = b.iter().map(pick).collect();
    let orient = |p: [f64; 2], q: [f64; 2], r: [f64; 2]| (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
    let e2 = eps * eps;
    let cross2 = |p: [f64; 2], q: [f64; 2], r: [f64; 2], s: [f64; 2]| {
        let (d1, d2, d3, d4) = (orient(r, s, p), orient(r, s, q), orient(p, q, r), orient(p, q, s));
        ((d1 > e2 && d2 < -e2) || (d1 < -e2 && d2 > e2)) && ((d3 > e2 && d4 < -e2) || (d3 < -e2 && d4 > e2))
    };
    let g = |v: &[[f64; 2]], k: usize| v.get(k % 3).copied().unwrap_or([f64::NAN; 2]);
    for k in 0..3 {
        for l in 0..3 {
            if cross2(g(&a2, k), g(&a2, k + 1), g(&b2, l), g(&b2, l + 1)) {
                return true;
            }
        }
    }
    let inside = |p: [f64; 2], t: &[[f64; 2]]| {
        let s = js::sign(orient(g(t, 0), g(t, 1), g(t, 2)));
        s * orient(g(t, 0), g(t, 1), p) > e2 && s * orient(g(t, 1), g(t, 2), p) > e2 && s * orient(g(t, 2), g(t, 0), p) > e2
    };
    let c = |t: &[[f64; 2]]| [(g(t, 0)[0] + g(t, 1)[0] + g(t, 2)[0]) / 3.0, (g(t, 0)[1] + g(t, 1)[1] + g(t, 2)[1]) / 3.0];
    inside(c(&a2), &b2) || inside(c(&b2), &a2)
}

// ---------------------------------------------------------------- files

/// `stlBinary(mesh, header)`: triangles as given, normals from the winding.
pub fn stl_binary(mesh: &Mesh, header: &str) -> Vec<u8> {
    let p = &mesh.positions;
    let nt = mesh.indices.len() / 3;
    let mut out = Vec::with_capacity(nt.saturating_mul(50).saturating_add(84));
    let mut head = [0u8; 80];
    for (d, u) in head.iter_mut().zip(header.encode_utf16().take(79)) {
        *d = u8::try_from(u & 0x7f).unwrap_or(0);
    }
    out.extend_from_slice(&head);
    out.extend_from_slice(&u32::try_from(nt).unwrap_or(u32::MAX).to_le_bytes());
    let iv = |k: usize| -> usize { mesh.indices.get(k).and_then(|&x| usize::try_from(x).ok()).unwrap_or(0) };
    for t in 0..nt {
        let (a, b, c) = (iv(t * 3), iv(t * 3 + 1), iv(t * 3 + 2));
        let n = tri_normal(p, a, b, c);
        let l0 = js::hypot(&n);
        let l = if l0 != 0.0 && !l0.is_nan() { l0 } else { 1.0 };
        for v in [n[0] / l, n[1] / l, n[2] / l] {
            out.extend_from_slice(&f32_of(v).to_le_bytes());
        }
        for v in [a, b, c] {
            for k in 0..3usize {
                out.extend_from_slice(&f32_of(p.get(v * 3 + k).copied().unwrap_or(f64::NAN)).to_le_bytes());
            }
        }
        out.extend_from_slice(&[0, 0]);
    }
    out
}

fn crc_table() -> &'static [u32; 256] {
    static T: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0u32; 256];
        for (n, slot) in t.iter_mut().enumerate() {
            let mut c = u32::try_from(n).unwrap_or(0);
            for _ in 0..8 {
                c = if c & 1 == 1 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        t
    })
}

/// `crc32(bytes)`.
pub fn crc32(bytes: &[u8]) -> u32 {
    let t = crc_table();
    let mut c = 0xffff_ffffu32;
    for &b in bytes {
        c = t.get(usize::from(u8::try_from((c ^ u32::from(b)) & 0xff).unwrap_or(0))).copied().unwrap_or(0) ^ (c >> 8);
    }
    c ^ 0xffff_ffff
}

/// `zipStored(files)`: a zip with every entry stored (no compression), UTF-8 names.
pub fn zip_stored(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut parts: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    let u16le = |x: usize| u16::try_from(x).unwrap_or(u16::MAX).to_le_bytes();
    let u32le = |x: usize| u32::try_from(x).unwrap_or(u32::MAX).to_le_bytes();
    let mut offset = 0usize;
    for (name, data) in files {
        let nb = name.as_bytes();
        let crc = crc32(data);
        let mut lh = [0u8; 30];
        lh[0..4].copy_from_slice(&0x0403_4b50u32.to_le_bytes());
        lh[4..6].copy_from_slice(&20u16.to_le_bytes());
        lh[6..8].copy_from_slice(&0x0800u16.to_le_bytes());
        lh[12..14].copy_from_slice(&0x21u16.to_le_bytes());
        lh[14..18].copy_from_slice(&crc.to_le_bytes());
        lh[18..22].copy_from_slice(&u32le(data.len()));
        lh[22..26].copy_from_slice(&u32le(data.len()));
        lh[26..28].copy_from_slice(&u16le(nb.len()));
        parts.extend_from_slice(&lh);
        parts.extend_from_slice(nb);
        parts.extend_from_slice(data);
        let mut ch = [0u8; 46];
        ch[0..4].copy_from_slice(&0x0201_4b50u32.to_le_bytes());
        ch[4..6].copy_from_slice(&20u16.to_le_bytes());
        ch[6..8].copy_from_slice(&20u16.to_le_bytes());
        ch[8..10].copy_from_slice(&0x0800u16.to_le_bytes());
        ch[14..16].copy_from_slice(&0x21u16.to_le_bytes());
        ch[16..20].copy_from_slice(&crc.to_le_bytes());
        ch[20..24].copy_from_slice(&u32le(data.len()));
        ch[24..28].copy_from_slice(&u32le(data.len()));
        ch[28..30].copy_from_slice(&u16le(nb.len()));
        ch[42..46].copy_from_slice(&u32le(offset));
        central.extend_from_slice(&ch);
        central.extend_from_slice(nb);
        offset = offset.saturating_add(30).saturating_add(nb.len()).saturating_add(data.len());
    }
    let mut end = [0u8; 22];
    end[0..4].copy_from_slice(&0x0605_4b50u32.to_le_bytes());
    end[8..10].copy_from_slice(&u16le(files.len()));
    end[10..12].copy_from_slice(&u16le(files.len()));
    end[12..16].copy_from_slice(&u32le(central.len()));
    end[16..20].copy_from_slice(&u32le(offset));
    let mut out = parts;
    out.extend_from_slice(&central);
    out.extend_from_slice(&end);
    out
}

/// `xmlEsc(s)`.
fn xml_esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' => {}
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}
/// `num(v)`: 10 significant digits, as a number's text.
fn num10(v: f64, out: &mut String) {
    let r = if v.abs() < 1e-12 { 0.0 } else { v };
    js::push_num(out, precision_num(r, 10));
}

/// A colour as 3MF keeps it: a #rrggbb in lower case, anything else the neutral grey.
fn swatch(c: Option<&String>) -> String {
    match c {
        Some(c) if c.len() == 7 && c.starts_with('#') && c.bytes().skip(1).all(|b| b.is_ascii_hexdigit()) => c.to_ascii_lowercase(),
        _ => "#c8c8c8".into(),
    }
}

/// `threeMF(mesh, {name, app})`: 3MF core, one object, millimetres, colours as one basematerials group; stored zip.
pub fn three_mf(mesh: &Mesh, name: &str, app: &str) -> Vec<u8> {
    let p = &mesh.positions;
    let nv = p.len() / 3;
    let nt = mesh.indices.len() / 3;
    let cols = mesh.colors.as_ref().filter(|c| c.iter().any(Option::is_some));
    let mut palette: Vec<String> = Vec::new();
    if let Some(cols) = cols {
        for c in cols {
            let k = swatch(c.as_ref());
            if !palette.contains(&k) {
                palette.push(k);
            }
        }
    }
    let mut out = String::with_capacity(nv.saturating_mul(60).saturating_add(nt.saturating_mul(50)).saturating_add(1024));
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<model unit=\"millimeter\" xml:lang=\"en-US\" xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">\n");
    out.push_str(&format!(
        " <metadata name=\"Title\">{}</metadata>\n <metadata name=\"Application\">{}</metadata>\n <resources>\n",
        xml_esc(name),
        xml_esc(app)
    ));
    if !palette.is_empty() {
        out.push_str("  <basematerials id=\"1\">\n");
        for c in &palette {
            out.push_str(&format!("   <base name=\"{c}\" displaycolor=\"{}FF\"/>\n", c.to_uppercase()));
        }
        out.push_str("  </basematerials>\n");
    }
    out.push_str(&format!(
        "  <object id=\"2\" type=\"model\" name=\"{}\"{}>\n   <mesh>\n    <vertices>\n",
        xml_esc(name),
        if palette.is_empty() { "" } else { " pid=\"1\" pindex=\"0\"" }
    ));
    for i in 0..nv {
        out.push_str("     <vertex x=\"");
        num10(p.get(i * 3).copied().unwrap_or(f64::NAN), &mut out);
        out.push_str("\" y=\"");
        num10(p.get(i * 3 + 1).copied().unwrap_or(f64::NAN), &mut out);
        out.push_str("\" z=\"");
        num10(p.get(i * 3 + 2).copied().unwrap_or(f64::NAN), &mut out);
        out.push_str("\"/>\n");
    }
    out.push_str("    </vertices>\n    <triangles>\n");
    for t in 0..nt {
        let g = |k: usize| mesh.indices.get(t * 3 + k).copied().unwrap_or(0);
        out.push_str(&format!("     <triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"", g(0), g(1), g(2)));
        if let Some(cols) = cols {
            let k = swatch(cols.get(t).and_then(Option::as_ref));
            let pi = palette.iter().position(|x| *x == k).map_or_else(|| "undefined".into(), |i| i.to_string());
            out.push_str(&format!(" pid=\"1\" p1=\"{pi}\""));
        }
        out.push_str("/>\n");
    }
    out.push_str("    </triangles>\n   </mesh>\n  </object>\n </resources>\n <build>\n  <item objectid=\"2\"/>\n </build>\n</model>\n");
    let types = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/></Types>\n";
    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Target=\"/3D/3dmodel.model\" Id=\"rel0\" Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/></Relationships>\n";
    zip_stored(&[("[Content_Types].xml", types.as_bytes()), ("_rels/.rels", rels.as_bytes()), ("3D/3dmodel.model", out.as_bytes())])
}

/// The mesh as JSON (what `canon(rep.mesh)` gives).
pub fn mesh_value(m: &Mesh) -> Value {
    json!({
        "positions": m.positions.iter().map(|&x| cc_m3d::file::jn(x)).collect::<Vec<_>>(),
        "indices": m.indices,
        "colors": m.colors.as_ref().map(|c| c.iter().map(|x| x.clone().map_or(Value::Null, Value::String)).collect::<Vec<_>>()),
        "degenerate": m.degenerate,
    })
}
