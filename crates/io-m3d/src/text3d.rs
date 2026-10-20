//! text3d.js — 3D text (0.4 M11): letters as faces, flat or pushed up to a depth, made into a component the app
//! places. The letters are the single-stroke font ([`crate::strokefont`]) given a pen width: every stroke segment
//! becomes a thin quad and every joint a small disc, and cc-manifold's CrossSection unions them into outlines with
//! holes (an "o" keeps its hole), simplified at height · 1e-4; depth > 0 extrudes them (Manifold, as solids.js does),
//! flat sides merged into single faces.
//!
//!   text_outlines(text, height, weight) → the CrossSection in text coordinates (x along the text from the left of
//!        the first line's baseline, y up; lines below each other, 1.6 heights apart; at most 50 lines).
//!   text_def(model, text, height, depth, weight, name) → a new component definition's id: faces facing +z (flat) or
//!        a closed solid from z = 0 to depth. Call it inside `Model::change`.

use cc_m3d::Model;
use cc_m3d::edges::merge_coplanar;
use cc_m3d::meshin::{PolyIn, mesh_def_in};
use cc_m3d::vec::P3;
use cc_manifold::{CrossSection, FillRule, Manifold};

use crate::Res;
use crate::jsx::{cos, sin};

/// Most points the letters may make before the union (400 characters of the densest glyphs stay far below).
const MAX_POINTS: usize = 4_000_000;

/// `textOutlines(M, text, {height, weight})`.
pub fn text_outlines(text: &str, height: f64, weight: f64) -> Res<CrossSection> {
    let r = (height * weight) / 2.0;
    let mut polys: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut points = 0usize;
    let disc = |c: [f64; 2]| -> Vec<[f64; 2]> {
        (0..12u32)
            .map(|i| {
                let a = (f64::from(i) * std::f64::consts::PI) / 6.0;
                [c[0] + r * cos(a), c[1] + r * sin(a)]
            })
            .collect()
    };
    for (k, line) in text.split('\n').take(50).enumerate() {
        // split(/\r?\n/)
        let line = line.strip_suffix('\r').unwrap_or(line);
        let dy = -(k as f64) * height * 1.6;
        for pl in crate::strokefont::text_strokes(line, height, 1.0, 0.0).lines {
            let p: Vec<[f64; 2]> = pl.iter().map(|&[x, y]| [x, y + dy]).collect();
            for &q in &p {
                polys.push(disc(q));
            }
            for w in p.windows(2) {
                let [a, b] = [w.first().copied().unwrap_or_default(), w.get(1).copied().unwrap_or_default()];
                let (dx, dyy) = (b[0] - a[0], b[1] - a[1]);
                let l = cc_m3d::js::hypot2(dx, dyy);
                if l < 1e-12 {
                    continue;
                }
                let (nx, ny) = ((-dyy / l) * r, (dx / l) * r);
                // counter-clockwise, so the Positive fill rule unions everything
                polys.push(vec![[a[0] - nx, a[1] - ny], [b[0] - nx, b[1] - ny], [b[0] + nx, b[1] + ny], [a[0] + nx, a[1] + ny]]);
            }
            points = points.saturating_add(p.len().saturating_mul(16));
            if points > MAX_POINTS {
                return Err(cc_m3d::Error::Msg("The text is too long.".into()));
            }
        }
    }
    let cs = CrossSection::try_new(&polys, FillRule::Positive).map_err(clip_err)?;
    cs.try_simplify(height * 1e-4).map_err(clip_err)
}

/// A Clipper2 error (coordinates past its range, too many points), which the wasm throws as an exception.
fn clip_err(_: cc_manifold::ClipError) -> cc_m3d::Error {
    cc_m3d::Error::Msg("The letters are too large to make.".into())
}

/// `Math.round(v * 1e6) / 1e6`: back from Manifold's 32-bit floats onto a 1 µm grid.
fn g6(v: f64) -> f64 {
    cc_m3d::js::round(v * 1e6) / 1e6
}

/// `textDef(M, model, text, {height, depth, weight, name})` → the new component definition's id.
pub fn text_def(model: &mut Model, text: &str, height: f64, depth: f64, weight: f64, name: &str) -> Res<String> {
    let cs = text_outlines(text, height, weight)?;
    let mut polys: Vec<PolyIn> = Vec::new();
    if depth > 0.0 {
        let man = Manifold::extrude(&cs.to_polygons(), depth, 0, 0.0, [1.0, 1.0]);
        let g = man.get_mesh();
        let np = usize::try_from(g.num_prop).unwrap_or(3);
        let at = |i: u32, k: usize| -> f64 {
            let idx = usize::try_from(i).unwrap_or(usize::MAX).saturating_mul(np).saturating_add(k);
            g6(f64::from(g.vert_properties.get(idx).copied().unwrap_or(f32::NAN)))
        };
        let p = |i: u32| -> P3 { [at(i, 0), at(i, 1), at(i, 2)] };
        for [a, b, c] in g.tri_verts.as_chunks::<3>().0 {
            polys.push(PolyIn::outline(vec![p(*a), p(*b), p(*c)]));
        }
    } else {
        // one face per letter piece, with its holes
        for piece in cs.try_decompose().map_err(clip_err)? {
            let loops: Vec<Vec<P3>> = piece.to_polygons().iter().map(|l| l.iter().map(|&[x, y]| [g6(x), g6(y), 0.0]).collect()).collect();
            if !loops.is_empty() {
                polys.push(PolyIn { loops, color: None });
            }
        }
    }
    let first = text.split('\n').next().unwrap_or("");
    let first = first.strip_suffix('\r').unwrap_or(first);
    let base = if !name.is_empty() {
        name.to_string()
    } else {
        let s = cc_m3d::js::slice16(first, 40);
        if s.is_empty() { "Text".to_string() } else { s }
    };
    let nm = model.unique_name(&base);
    let id = mesh_def_in(model, &polys, Some(&nm), "component", &[]);
    if depth > 0.0
        && let Some(d) = model.defs.get_mut(&id)
    {
        let soft: Vec<f64> = d.edges.iter().filter(|(_, e)| e.soft).map(|(id, _)| id).collect();
        merge_coplanar(d, &soft);
    }
    Ok(id)
}
