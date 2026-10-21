//! The material library's textures, drawn by code: the Rust form of 0.5's `render/materials.js` (the `tex` specs and
//! `planarUV`) and `render/textures.js` (seeded tileable noise and twelve generators), at exact parity with the frozen
//! JS — the same id and size give the same bytes (`forks/tools/texdiff.mjs` compares every material's albedo, height
//! and roughness). The library is CadCraft's own work (CC0): every texture is made here from numbers.
//!
//! Numbers follow the JS: `Math.sin` is fdlibm (cc-m3d `jsmath`), `Math.hypot` V8's (cc-m3d `js`), the albedo goes
//! through a `Uint8ClampedArray` (clamped, rounded half to even), height and roughness through a `Float32Array`.

#![allow(clippy::manual_range_contains)]

use cc_m3d::js::{hypot2, round as js_round};
use cc_m3d::jsmath::fd_sin;

/// What a texture is drawn from (a material's `tex`, materials.js). Fields left `None` take the generator's default.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TexSpec {
    pub kind: &'static str,
    pub color: &'static str,
    /// the real size one copy covers, metres [width, height]
    pub size: [f64; 2],
    pub seed: u32,
    pub mortar: Option<&'static str>,
    pub grout: Option<&'static str>,
    pub boards: Option<f64>,
    pub rows: Option<f64>,
    pub tabs: Option<f64>,
    pub cells: Option<f64>,
    pub tiles: Option<f64>,
    pub panels: Option<f64>,
    pub figure: Option<f64>,
    pub rough: Option<f64>,
    pub grain: bool,
    pub ashlar: bool,
    pub ties: bool,
}

const fn spec(kind: &'static str, color: &'static str, size: [f64; 2], seed: u32) -> TexSpec {
    TexSpec {
        kind,
        color,
        size,
        seed,
        mortar: None,
        grout: None,
        boards: None,
        rows: None,
        tabs: None,
        cells: None,
        tiles: None,
        panels: None,
        figure: None,
        rough: None,
        grain: false,
        ashlar: false,
        ties: false,
    }
}

/// The texture of a library material (materials.js MATERIALS' `tex`), or None for one drawn in its colour only (glass,
/// metal) or an unknown id.
pub fn tex_spec(id: &str) -> Option<TexSpec> {
    let s = |kind, color, size, seed| spec(kind, color, size, seed);
    Some(match id {
        "brick-red" => TexSpec { mortar: Some("#bdb5a8"), ..s("brick", "#9c4a35", [0.9, 0.6], 11) },
        "brick-brown" => TexSpec { mortar: Some("#a9a095"), ..s("brick", "#6e4632", [0.9, 0.6], 12) },
        "brick-buff" => TexSpec { mortar: Some("#d8d0c2"), ..s("brick", "#c9a777", [0.9, 0.6], 13) },
        "brick-painted" => TexSpec { mortar: Some("#d6d0c5"), ..s("brick", "#e8e4dc", [0.9, 0.6], 14) },
        "block-gray" => s("block", "#a7a49e", [0.8, 0.8], 15),
        "siding-white" => TexSpec { boards: Some(8.0), ..s("siding", "#eeeae2", [1.2, 1.2], 21) },
        "siding-gray" => TexSpec { boards: Some(8.0), ..s("siding", "#9aa3a6", [1.2, 1.2], 22) },
        "siding-blue" => TexSpec { boards: Some(8.0), ..s("siding", "#748b9e", [1.2, 1.2], 23) },
        "siding-cedar" => TexSpec { boards: Some(8.0), grain: true, ..s("siding", "#a0673f", [1.2, 1.2], 24) },
        "shingles-gray" => TexSpec { rows: Some(8.0), tabs: Some(6.0), ..s("shingles", "#5f6164", [2.0, 1.14], 31) },
        "shingles-brown" => TexSpec { rows: Some(8.0), tabs: Some(6.0), ..s("shingles", "#5a4535", [2.0, 1.14], 32) },
        "shingles-cedar" => TexSpec { rows: Some(8.0), tabs: Some(12.0), ..s("shingles", "#8a5a3b", [1.8, 1.0], 33) },
        "roof-metal" => TexSpec { panels: Some(4.0), ..s("seam", "#4b5a63", [1.6, 1.6], 34) },
        "tile-roof" => TexSpec { rows: Some(6.0), tabs: Some(5.0), ..s("shingles", "#b0603e", [1.5, 1.5], 35) },
        "wood-oak" => TexSpec { boards: Some(6.0), ..s("planks", "#b58552", [2.4, 0.84], 41) },
        "wood-walnut" => TexSpec { boards: Some(6.0), ..s("planks", "#6b4a32", [2.4, 0.84], 42) },
        "wood-pine" => TexSpec { boards: Some(6.0), figure: Some(0.8), ..s("planks", "#d4b07a", [2.4, 0.84], 43) },
        "wood-deck" => TexSpec { boards: Some(4.0), rough: Some(0.8), ..s("planks", "#9b8f7f", [2.4, 0.56], 44) },
        "stone-field" => TexSpec { cells: Some(7.0), ..s("stone", "#8f8a80", [1.2, 1.2], 51) },
        "stone-limestone" => TexSpec { ashlar: true, rows: Some(5.0), ..s("stone", "#cdbfa3", [1.5, 1.5], 52) },
        "stone-slate" => TexSpec { ashlar: true, mortar: Some("#3f4246"), rows: Some(6.0), ..s("stone", "#555a60", [1.2, 1.2], 53) },
        "tile-white" => TexSpec { tiles: Some(4.0), rough: Some(0.2), ..s("tile", "#eceae4", [1.2, 1.2], 61) },
        "tile-terracotta" => TexSpec { grout: Some("#c9bfae"), tiles: Some(4.0), rough: Some(0.6), ..s("tile", "#b8653f", [1.2, 1.2], 62) },
        "tile-gray" => TexSpec { grout: Some("#a5a39e"), tiles: Some(2.0), rough: Some(0.35), ..s("tile", "#83868a", [1.2, 1.2], 63) },
        "concrete" => s("concrete", "#b3afa8", [2.0, 2.0], 71),
        "concrete-ties" => TexSpec { ties: true, ..s("concrete", "#aaa7a1", [1.2, 1.2], 72) },
        "stucco-white" => s("stucco", "#e7e1d5", [1.0, 1.0], 81),
        "stucco-sand" => s("stucco", "#d6c3a0", [1.0, 1.0], 82),
        "grass" => s("grass", "#5d7f3a", [2.0, 2.0], 91),
        "gravel" => s("gravel", "#9a948a", [1.0, 1.0], 92),
        _ => return None,
    })
}

/// The ids of the library's textured materials, in the library's order (materials.js MATERIALS with a `tex`).
pub const TEXTURED: [&str; 30] = [
    "brick-red",
    "brick-brown",
    "brick-buff",
    "brick-painted",
    "block-gray",
    "siding-white",
    "siding-gray",
    "siding-blue",
    "siding-cedar",
    "shingles-gray",
    "shingles-brown",
    "shingles-cedar",
    "roof-metal",
    "tile-roof",
    "wood-oak",
    "wood-walnut",
    "wood-pine",
    "wood-deck",
    "stone-field",
    "stone-limestone",
    "stone-slate",
    "tile-white",
    "tile-terracotta",
    "tile-gray",
    "concrete",
    "concrete-ties",
    "stucco-white",
    "stucco-sand",
    "grass",
    "gravel",
];

/// materials.js `planarUV(p, n, up)`: where a point of a surface falls on its texture, metres — u level along the
/// surface, v up it; a level surface takes u along X (Z up), v along up × u.
pub fn planar_uv(p: [f64; 3], n: [f64; 3], up: [f64; 3]) -> [f64; 2] {
    let nl = {
        let l = cc_m3d::js::hypot3(n[0], n[1], n[2]);
        if l == 0.0 || l.is_nan() { 1.0 } else { l }
    };
    let (nx, ny, nz) = (n[0] / nl, n[1] / nl, n[2] / nl);
    let ud = nx * up[0] + ny * up[1] + nz * up[2];
    if ud.abs() > 0.999 {
        let (ux, uy, uz) = if up[0].abs() < 0.9 { (1.0, 0.0, 0.0) } else { (0.0, 1.0, 0.0) };
        let (vx, vy, vz) = (up[1] * uz - up[2] * uy, up[2] * ux - up[0] * uz, up[0] * uy - up[1] * ux);
        return [p[0] * ux + p[1] * uy + p[2] * uz, p[0] * vx + p[1] * vy + p[2] * vz];
    }
    let (mut ux, mut uy, mut uz) = (up[1] * nz - up[2] * ny, up[2] * nx - up[0] * nz, up[0] * ny - up[1] * nx);
    let ul = cc_m3d::js::hypot3(ux, uy, uz);
    ux /= ul;
    uy /= ul;
    uz /= ul;
    let (vx, vy, vz) = (ny * uz - nz * uy, nz * ux - nx * uz, nx * uy - ny * ux);
    [p[0] * ux + p[1] * uy + p[2] * uz, p[0] * vx + p[1] * vy + p[2] * vz]
}

/// A texture: `w × h` texels, rows from v = 0 (the first row is the bottom of the tile, as three.js reads a
/// DataTexture); albedo RGBA, height and roughness 0…1.
#[derive(Clone, Debug, PartialEq)]
pub struct Texture {
    pub w: usize,
    pub h: usize,
    pub albedo: Vec<u8>,
    pub height: Vec<f32>,
    pub rough: Vec<f32>,
}

// ---- seeded randomness and tileable noise ----

/// textures.js `rng(seed)` (mulberry32): the next number in 0…1 each call.
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Rng {
        Rng(seed)
    }
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        f64::from(t ^ (t >> 14)) / 4_294_967_296.0
    }
}

/// `ToInt32` of a JS number as the u32 with the same bits (what `Math.imul` and `^` see).
fn to_u32(x: f64) -> u32 {
    if !x.is_finite() {
        return 0;
    }
    if x.abs() < 9.0e15 {
        // exact in i64; the cast to u32 keeps the low 32 bits (mod 2^32), as ToInt32 does
        return x.trunc() as i64 as u32;
    }
    let m = x.trunc().rem_euclid(4_294_967_296.0);
    m as u32
}

fn hash2(x: f64, y: f64, s: f64) -> f64 {
    let h = to_u32(x)
        .wrapping_mul(374_761_393)
        .wrapping_add(to_u32(y).wrapping_mul(668_265_263))
        .wrapping_add(to_u32(s).wrapping_mul(2_147_483_647));
    let h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    f64::from(h ^ (h >> 16)) / 4_294_967_296.0
}

fn fade(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}
/// JS `((a % n) + n) % n`.
fn jmod(a: f64, n: f64) -> f64 {
    // the common case without fmod: 0 ≤ a < n gives a (+0 for −0, as the JS's sum does)
    if a >= 0.0 && a < n {
        return a + 0.0;
    }
    ((a % n) + n) % n
}

fn vnoise(x: f64, y: f64, px: f64, seed: f64, py: f64) -> f64 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (fade(x - x0), fade(y - y0));
    let a = hash2(jmod(x0, px), jmod(y0, py), seed);
    let b = hash2(jmod(x0 + 1.0, px), jmod(y0, py), seed);
    let c = hash2(jmod(x0, px), jmod(y0 + 1.0, py), seed);
    let d = hash2(jmod(x0 + 1.0, px), jmod(y0 + 1.0, py), seed);
    a + (b - a) * fx + (c - a) * fy + (a - b - c + d) * fx * fy
}

/// textures.js `fbm(u, v, base, octaves, seed, gain = 0.5)`.
pub fn fbm(u: f64, v: f64, base: f64, octaves: u32, seed: f64) -> f64 {
    let (mut s, mut amp, mut norm, mut f) = (0.0, 1.0, 0.0, base);
    for o in 0..octaves {
        s += amp * vnoise(u * f, v * f, f, seed + f64::from(o) * 101.0, f);
        norm += amp;
        amp *= 0.5;
        f *= 2.0;
    }
    s / norm
}

/// textures.js `fbm2(u, v, bu, bv, octaves, seed, gain = 0.5)`: stretched (wood grain).
pub fn fbm2(u: f64, v: f64, bu: f64, bv: f64, octaves: u32, seed: f64) -> f64 {
    let (mut s, mut amp, mut norm, mut fu, mut fv) = (0.0, 1.0, 0.0, bu, bv);
    for o in 0..octaves {
        s += amp * vnoise(u * fu, v * fv, fu, seed + f64::from(o) * 101.0, fv);
        norm += amp;
        amp *= 0.5;
        fu *= 2.0;
        fv *= 2.0;
    }
    s / norm
}

type Rgb = [f64; 3];

/// `parseInt` of the three pairs of '#rrggbb' (NaN for what does not parse, as the JS).
fn hex(c: &str) -> Rgb {
    let p = |a: usize| c.get(a..a + 2).and_then(|s| u8::from_str_radix(s, 16).ok()).map_or(f64::NAN, f64::from);
    [p(1), p(3), p(5)]
}
fn clamp01(x: f64) -> f64 {
    x.clamp(0.0, 1.0)
}
fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}
fn shade(c: Rgb, k: f64) -> Rgb {
    [c[0] * k, c[1] * k, c[2] * k]
}
/// A `Uint8ClampedArray` store: clamped to 0…255, rounded half to even, NaN → 0.
fn clamped_u8(x: f64) -> u8 {
    if x.is_nan() || x <= 0.0 {
        0
    } else if x >= 255.0 {
        255
    } else {
        x.round_ties_even() as u8
    }
}
/// `arr[i]` of a JS array: undefined (NaN in arithmetic) past its end.
fn at(a: &[f64], i: f64) -> f64 {
    if i >= 0.0 && i.fract() == 0.0 { a.get(i as usize).copied().unwrap_or(f64::NAN) } else { f64::NAN }
}

/// textures.js `paint(size, aspect, fill)`.
fn paint(size: usize, aspect: f64, mut fill: impl FnMut(f64, f64) -> (Rgb, f64, f64)) -> Texture {
    let w = size;
    let h = js_round(size as f64 * aspect).clamp(4.0, 4096.0) as usize;
    let n = w.saturating_mul(h);
    let mut t = Texture { w, h, albedo: vec![0; n.saturating_mul(4)], height: vec![0.0; n], rough: vec![0.0; n] };
    let mut i = 0usize;
    for y in 0..h {
        let v = (y as f64 + 0.5) / h as f64;
        for x in 0..w {
            let u = (x as f64 + 0.5) / w as f64;
            let (c, ht, r) = fill(u, v);
            if let Some(px) = t.albedo.get_mut(i * 4..i * 4 + 4) {
                px.copy_from_slice(&[clamped_u8(c[0]), clamped_u8(c[1]), clamped_u8(c[2]), 255]);
            }
            if let Some(o) = t.height.get_mut(i) {
                *o = clamp01(ht) as f32;
            }
            if let Some(o) = t.rough.get_mut(i) {
                *o = clamp01(r) as f32;
            }
            i += 1;
        }
    }
    t
}

/// The options a generator reads (a spec with its kind's adjustments: block → brick, gravel → stone).
#[derive(Clone, Copy)]
struct Opt {
    s: TexSpec,
    aspect: f64,
    cols: Option<f64>,
    joint: Option<f64>,
}

fn brick(o: Opt, size: usize, seed: u32) -> Texture {
    let s = o.s;
    let cols = o.cols.unwrap_or(4.0);
    let rows = s.rows.unwrap_or(8.0);
    let [ww, hh] = s.size;
    let m = o.joint.unwrap_or(0.01);
    let (ju, jv) = ((m * cols) / ww, (m * rows) / hh);
    let base = hex(s.color);
    let mortar = hex(s.mortar.unwrap_or("#b9b2a6"));
    let mut r = Rng::new(seed);
    let tint: Vec<f64> = (0..(cols * rows) as usize).map(|_| 0.82 + r.next() * 0.3).collect();
    let sd = f64::from(seed);
    paint(size, o.aspect, |u, v| {
        let row = (v * rows).floor();
        let fv = v * rows - row;
        let cu = u * cols + (row % 2.0) * 0.5;
        let col = cu.floor();
        let fu = cu - col;
        let n = fbm(u, v, 16.0, 4, sd);
        if fv < jv / 2.0 || fv > 1.0 - jv / 2.0 || fu < ju / 2.0 || fu > 1.0 - ju / 2.0 {
            let k = 0.9 + n * 0.2;
            return (shade(mortar, k), 0.15 + n * 0.1, 0.95);
        }
        let k = at(&tint, row * cols + jmod(col, cols)) * (0.88 + n * 0.24);
        let speck = if fbm(u, v, 64.0, 2, sd + 7.0) > 0.72 { 0.8 } else { 1.0 };
        (shade(base, k * speck), 0.75 + n * 0.25, 0.85)
    })
}

fn planks(o: Opt, size: usize, seed: u32) -> Texture {
    let s = o.s;
    let boards = s.boards.unwrap_or(6.0);
    let base = hex(s.color);
    let dark = hex("#000000");
    let mut r = Rng::new(seed);
    let tone: Vec<f64> = (0..(boards * 3.0) as usize).map(|_| 0.8 + r.next() * 0.35).collect();
    let cut: Vec<f64> = (0..boards as usize).map(|_| r.next()).collect();
    let figure = s.figure.unwrap_or(0.6);
    let rough = s.rough.unwrap_or(0.6);
    let sd = f64::from(seed);
    paint(size, o.aspect, |u, v| {
        let b = (v * boards).floor();
        let fv = v * boards - b;
        let cb = at(&cut, b);
        let seg = (jmod(u + cb, 1.0) * 2.0).floor();
        let fu = jmod(u + cb, 1.0) * 2.0 - seg;
        let gap = fv < 0.035 || fv > 0.965 || fu < 0.004 || fu > 0.996;
        let grain = fbm2(u, v, 2.0, 32.0, 3, sd + b * 13.0);
        let rings = 0.5 + 0.5 * fd_sin((v * boards * 14.0 + grain * 9.0 + b * 3.1) * std::f64::consts::PI);
        let k = at(&tone, b * 3.0 + seg) * (0.85 + 0.25 * rings * figure);
        let c = mix(shade(base, k), dark, (1.0 - rings) * 0.18 * figure);
        if gap {
            return (shade(c, 0.45), 0.1, 0.9);
        }
        (c, 0.6 + 0.2 * rings, rough + grain * 0.1)
    })
}

fn siding(o: Opt, size: usize, seed: u32) -> Texture {
    let s = o.s;
    let boards = s.boards.unwrap_or(8.0);
    let base = hex(s.color);
    let rough = s.rough.unwrap_or(0.55);
    let sd = f64::from(seed);
    paint(size, o.aspect, |u, v| {
        let b = (v * boards).floor();
        let fv = v * boards - b;
        let n = fbm(u, v, 24.0, 3, sd);
        let grain = if s.grain { fbm2(u, v, 2.0, 48.0, 2, sd + 3.0) } else { 0.5 };
        if fv < 0.07 {
            return (shade(base, 0.55 + fv * 3.0), 0.05 + fv, 0.8);
        }
        (shade(base, 0.92 + n * 0.1 + (grain - 0.5) * 0.12), 0.3 + fv * 0.6, rough)
    })
}

fn shingles(o: Opt, size: usize, seed: u32) -> Texture {
    let s = o.s;
    let rows = s.rows.unwrap_or(8.0);
    let tabs = s.tabs.unwrap_or(6.0);
    let base = hex(s.color);
    let mut r = Rng::new(seed);
    let tone: Vec<f64> = (0..(rows * tabs * 2.0) as usize).map(|_| 0.78 + r.next() * 0.4).collect();
    let len = tone.len() as f64;
    let sd = f64::from(seed);
    let sz = size as f64;
    paint(size, o.aspect, |u, v| {
        let rr = (v * rows).floor();
        let fv = v * rows - rr;
        let cu = u * tabs + (rr % 2.0) * 0.5;
        let t = cu.floor();
        let fu = cu - t;
        let grit = fbm(u, v, 48.0, 3, sd) * 0.35 + (hash2((u * sz).floor(), (v * sz).floor(), sd) - 0.5) * 0.25;
        let k = at(&tone, jmod(rr * tabs * 2.0 + jmod(t, tabs * 2.0), len));
        if fu < 0.03 || fu > 0.97 {
            return (shade(base, 0.35), 0.05, 0.95);
        }
        if fv < 0.12 {
            return (shade(base, 0.45 + fv * 2.5), 0.1 + fv * 3.0, 0.95);
        }
        (shade(base, k * (0.85 + grit)), 0.55 + fv * 0.4 + grit * 0.2, 0.92)
    })
}

fn stone(o: Opt, size: usize, seed: u32) -> Texture {
    let s = o.s;
    let base = hex(s.color);
    let mortar = hex(s.mortar.unwrap_or("#a9a39a"));
    let mut r = Rng::new(seed);
    let sd = f64::from(seed);
    if s.ashlar {
        let rows = s.rows.unwrap_or(5.0);
        let lens: Vec<Vec<f64>> = (0..rows as usize)
            .map(|_| {
                let mut a = Vec::new();
                let mut x = 0.0;
                // each piece is at least 0.18 long: at most 6 per row
                while x < 1.0 && a.len() < 64 {
                    let l = 0.18 + r.next() * 0.3;
                    a.push((x + l).min(1.0));
                    x += l;
                }
                if let Some(last) = a.last_mut() {
                    *last = 1.0;
                }
                a
            })
            .collect();
        let tone: Vec<f64> = (0..(rows * 8.0) as usize).map(|_| 0.8 + r.next() * 0.35).collect();
        return paint(size, o.aspect, |u, v| {
            let rr = (v * rows).floor();
            let fv = v * rows - rr;
            let empty = Vec::new();
            let ends = lens.get(rr as usize).unwrap_or(&empty);
            let mut i = 0usize;
            while i + 1 < ends.len() && ends.get(i).is_some_and(|e| u > *e) {
                i += 1;
            }
            let start = if i > 0 { ends.get(i - 1).copied().unwrap_or(f64::NAN) } else { 0.0 };
            let end = ends.get(i).copied().unwrap_or(f64::NAN);
            let du = (u - start).min(end - u) * rows;
            let n = fbm(u, v, 12.0, 5, sd);
            if fv < 0.05 || fv > 0.95 || du < 0.04 {
                return (shade(mortar, 0.85 + n * 0.2), 0.1, 0.95);
            }
            (shade(base, at(&tone, rr * 8.0 + (i % 8) as f64) * (0.8 + n * 0.35)), 0.7 + n * 0.3, 0.8)
        });
    }
    let cells = s.cells.unwrap_or(7.0);
    let nc = cells as usize;
    let mut pts: Vec<[f64; 3]> = Vec::with_capacity(nc * nc);
    for j in 0..nc {
        for i in 0..nc {
            let a = (i as f64 + 0.15 + r.next() * 0.7) / cells;
            let b = (j as f64 + 0.15 + r.next() * 0.7) / cells;
            let c = 0.75 + r.next() * 0.4;
            pts.push([a, b, c]);
        }
    }
    let joint = o.joint.unwrap_or(0.12);
    paint(size, o.aspect, |u, v| {
        let (mut d1, mut d2, mut k) = (9.0f64, 9.0f64, 1.0f64);
        let ci = (u * cells).floor();
        let cj = (v * cells).floor();
        for dj in -2i32..=2 {
            let j = cj + f64::from(dj);
            for di in -2i32..=2 {
                let i = ci + f64::from(di);
                let idx = jmod(j, cells) * cells + jmod(i, cells);
                let p = if idx >= 0.0 { pts.get(idx as usize).copied().unwrap_or([f64::NAN; 3]) } else { [f64::NAN; 3] };
                let dx = u - (p[0] + (i / cells).floor());
                let dy = v - (p[1] + (j / cells).floor());
                let d = hypot2(dx, dy);
                if d < d1 {
                    d2 = d1;
                    d1 = d;
                    k = p[2];
                } else if d < d2 {
                    d2 = d;
                }
            }
        }
        let edge = (d2 - d1) * cells;
        let n = fbm(u, v, 10.0, 5, sd);
        if edge < joint {
            return (shade(mortar, 0.8 + n * 0.25), 0.05 + edge, 0.95);
        }
        (shade(base, k * (0.78 + n * 0.4)), 0.55 + (edge * 0.8).min(0.45) * (0.7 + n * 0.3), 0.85)
    })
}

fn tile(o: Opt, size: usize, seed: u32) -> Texture {
    let s = o.s;
    let n0 = s.tiles.unwrap_or(4.0);
    let base = hex(s.color);
    let grout = hex(s.grout.unwrap_or("#cfcac2"));
    let mut r = Rng::new(seed);
    let tone: Vec<f64> = (0..(n0 * n0) as usize).map(|_| 0.9 + r.next() * 0.16).collect();
    let rough = s.rough.unwrap_or(0.25);
    let g = o.joint.unwrap_or(0.04);
    let sd = f64::from(seed);
    paint(size, o.aspect, |u, v| {
        let i = (u * n0).floor();
        let j = (v * n0).floor();
        let (fu, fv) = (u * n0 - i, v * n0 - j);
        let n = fbm(u, v, 20.0, 3, sd);
        if fu < g / 2.0 || fu > 1.0 - g / 2.0 || fv < g / 2.0 || fv > 1.0 - g / 2.0 {
            return (shade(grout, 0.9 + n * 0.15), 0.1, 0.95);
        }
        (shade(base, at(&tone, j * n0 + i) * (0.95 + n * 0.08)), 0.8, rough)
    })
}

fn concrete(o: Opt, size: usize, seed: u32) -> Texture {
    let s = o.s;
    let base = hex(s.color);
    let sd = f64::from(seed);
    let sz = size as f64;
    paint(size, o.aspect, |u, v| {
        let n = fbm(u, v, 4.0, 6, sd);
        let p = hash2((u * sz).floor(), (v * sz).floor(), sd + 1.0);
        let (mut k, mut h) = (0.86 + n * 0.26, 0.5 + n * 0.3);
        if p > 0.993 {
            k *= 0.6;
            h = 0.2;
        }
        if s.ties {
            let du = jmod(u * 2.0, 1.0) - 0.5;
            let dv = jmod(v * 2.0, 1.0) - 0.5;
            if hypot2(du, dv) < 0.025 {
                k *= 0.45;
                h = 0.05;
            }
        }
        (shade(base, k), h, 0.85)
    })
}

fn grass(o: Opt, size: usize, seed: u32) -> Texture {
    let base = hex(o.s.color);
    let dry = hex("#9c9a55");
    let sd = f64::from(seed);
    let sz = size as f64;
    paint(size, o.aspect, |u, v| {
        let blades = fbm2(u, v, 120.0, 32.0, 3, sd);
        let patch = fbm(u, v, 3.0, 4, sd + 5.0);
        let fine = hash2((u * sz).floor(), (v * sz).floor(), sd);
        let c = mix(base, dry, clamp01((patch - 0.55) * 2.5) * 0.6);
        (shade(c, 0.7 + blades * 0.45 + (fine - 0.5) * 0.18), blades, 0.95)
    })
}

fn stucco(o: Opt, size: usize, seed: u32) -> Texture {
    let base = hex(o.s.color);
    let sd = f64::from(seed);
    let sz = size as f64;
    paint(size, o.aspect, |u, v| {
        let n = fbm(u, v, 24.0, 4, sd);
        let f = hash2((u * sz).floor(), (v * sz).floor(), sd + 2.0);
        (shade(base, 0.9 + n * 0.12 + (f - 0.5) * 0.08), n * 0.7 + f * 0.3, 0.92)
    })
}

fn seam(o: Opt, size: usize, seed: u32) -> Texture {
    let panels = o.s.panels.unwrap_or(4.0);
    let base = hex(o.s.color);
    let sd = f64::from(seed);
    paint(size, o.aspect, |u, v| {
        let fu = jmod(u * panels, 1.0);
        let n = fbm(u, v, 8.0, 3, sd);
        if fu < 0.03 {
            return (shade(base, 1.15), 1.0, 0.35);
        }
        if fu < 0.06 {
            return (shade(base, 0.7), 0.4, 0.4);
        }
        (shade(base, 0.95 + n * 0.08), 0.5, 0.4)
    })
}

/// textures.js `makeTexture(spec, size)`: `size` texels across (16…1024), the height following the tile's real shape.
pub fn make_texture(spec: &TexSpec, size: usize) -> Texture {
    make_texture_aspect(spec, size, spec.size[1] / spec.size[0])
}

/// The same pattern on `size × size(×aspect)` texels: aspect 1 gives a square texture of the same tile (the 3D view's
/// texture array; the pattern is a function of the tile's u, v, so only the sampling of v changes).
pub fn make_texture_aspect(spec: &TexSpec, size: usize, aspect: f64) -> Texture {
    let size = size.clamp(16, 1024);
    let seed = spec.seed;
    let o = Opt { s: *spec, aspect, cols: None, joint: None };
    match spec.kind {
        "brick" => brick(o, size, seed),
        "block" => brick(
            Opt {
                s: TexSpec { rows: Some(spec.rows.unwrap_or(4.0)), mortar: Some(spec.mortar.unwrap_or("#9d9a94")), ..*spec },
                cols: Some(2.0),
                joint: Some(0.05),
                ..o
            },
            size,
            seed,
        ),
        "planks" => planks(o, size, seed),
        "siding" => siding(o, size, seed),
        "shingles" => shingles(o, size, seed),
        "stone" => stone(o, size, seed),
        "tile" => tile(o, size, seed),
        "concrete" => concrete(o, size, seed),
        "grass" => grass(o, size, seed),
        "stucco" => stucco(o, size, seed),
        "seam" => seam(o, size, seed),
        "gravel" => stone(
            Opt {
                s: TexSpec { cells: Some(spec.cells.unwrap_or(22.0)), mortar: Some(spec.mortar.unwrap_or("#6f6a62")), ..*spec },
                joint: Some(0.08),
                ..o
            },
            size,
            seed,
        ),
        _ => Texture { w: 0, h: 0, albedo: Vec::new(), height: Vec::new(), rough: Vec::new() },
    }
}

/// three-mats.js `swatchPixels(id, px)` for a textured material: the texture (128 across) sampled into `px × px`, its
/// lower-left corner, rows from the top; RGBA. None for a material without a texture.
pub fn swatch_pixels(id: &str, px: usize) -> Option<Vec<u8>> {
    let spec = tex_spec(id)?;
    let t = make_texture(&spec, 128);
    let px = px.clamp(1, 512);
    let k = t.w.min(t.h) as f64 / px as f64;
    let mut out = vec![0u8; px * px * 4];
    for y in 0..px {
        for x in 0..px {
            let sx = ((x as f64 * k).floor() as usize).min(t.w.saturating_sub(1));
            let sy = (((px - 1 - y) as f64 * k).floor() as usize).min(t.h.saturating_sub(1));
            let i = (sy * t.w + sx) * 4;
            let (Some(&[r, g, b]), Some(dst)) =
                (t.albedo.get(i..i + 3).and_then(|x| <&[u8; 3]>::try_from(x).ok()), out.get_mut((y * px + x) * 4..(y * px + x) * 4 + 4))
            else {
                continue;
            };
            dst.copy_from_slice(&[r, g, b, 255]);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_is_mulberry32() {
        // textures.js rng(11): the first three numbers
        let mut r = Rng::new(11);
        let a = [r.next(), r.next(), r.next()];
        assert!(a.iter().all(|x| (0.0..1.0).contains(x)));
        let mut r2 = Rng::new(11);
        assert_eq!(a[0], r2.next());
    }

    #[test]
    fn every_texture_tiles_its_size_and_seed() {
        for id in TEXTURED {
            let s = tex_spec(id).unwrap();
            let t = make_texture(&s, 32);
            assert_eq!(t.w, 32);
            assert_eq!(t.albedo.len(), t.w * t.h * 4);
            assert!(t.albedo.chunks(4).all(|p| p[3] == 255), "{id}");
            assert_eq!(make_texture(&s, 32), t, "{id}: the same spec gives the same pixels");
        }
        assert!(tex_spec("glass-clear").is_none());
    }

    #[test]
    fn level_and_upright_uvs() {
        assert_eq!(planar_uv([2.0, 3.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0]), [2.0, 3.0]);
        // a wall facing -Y: u along +X... v up
        let uv = planar_uv([2.0, 0.0, 1.5], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]);
        assert!((uv[1] - 1.5).abs() < 1e-12);
    }
}
