//! linetypes.js: linetypes and lineweights. A layer has `linetype` ('Continuous' by default) and `lineweight` (mm;
//! -3 default); an object may have `lt` (a name or 'ByBlock'; absent = by layer), `lw` (mm, -2 by block, -3 default)
//! and `lts` (its own linetype scale). `doc.linetypes` holds a file's patterns; `meta.ltscale` is LTSCALE.

use cc_cad::entity::Entity;
use cc_cad::omap::OMap;
use cc_cad::{Doc, Layer, js};

/// A layer table (doc.layers, or a viewport's overridden copy).
pub type Layers = OMap<String, Layer>;

use crate::geom::{PI, Prim};
use crate::hatch::hatch_is_metric_doc;

const BASE: [(&str, &[f64]); 8] = [
    ("DASHED", &[0.5, -0.25]),
    ("HIDDEN", &[0.25, -0.125]),
    ("CENTER", &[1.25, -0.25, 0.25, -0.25]),
    ("PHANTOM", &[1.25, -0.25, 0.25, -0.25, 0.25, -0.25]),
    ("DOT", &[0.0, -0.25]),
    ("DASHDOT", &[0.5, -0.25, 0.0, -0.25]),
    ("BORDER", &[0.5, -0.25, 0.5, -0.25, 0.0, -0.25]),
    ("DIVIDE", &[0.5, -0.25, 0.0, -0.25, 0.0, -0.25]),
];
pub const LINEWEIGHTS: [f64; 24] =
    [0.0, 0.05, 0.09, 0.13, 0.15, 0.18, 0.2, 0.25, 0.3, 0.35, 0.4, 0.5, 0.53, 0.6, 0.7, 0.8, 0.9, 1.0, 1.06, 1.2, 1.4, 1.58, 2.0, 2.11];
pub const DEFAULT_LW: f64 = 0.25;
const MAX_DASHES: f64 = 20000.0;

/// The standard linetype (inches) by upper-case name: NAME, NAME2 (half size), NAMEX2 (double size).
pub fn standard(name: &str) -> Option<Vec<f64>> {
    for (n, pat) in BASE {
        if name == n {
            return Some(pat.to_vec());
        }
        if name.strip_prefix(n) == Some("2") {
            return Some(pat.iter().map(|v| v * 0.5).collect());
        }
        if name.strip_prefix(n) == Some("X2") {
            return Some(pat.iter().map(|v| v * 2.0).collect());
        }
    }
    None
}
/// Every standard name in the JS order.
pub fn standard_names() -> Vec<String> {
    BASE.iter().flat_map(|(n, _)| [n.to_string(), format!("{n}2"), format!("{n}X2")]).collect()
}

fn is_cont(n: Option<&str>) -> bool {
    n.is_none_or(|s| s.is_empty() || s.eq_ignore_ascii_case("continuous"))
}
fn is_by_layer(n: Option<&str>) -> bool {
    n.is_none_or(|s| s.eq_ignore_ascii_case("bylayer"))
}
fn is_by_block(n: Option<&str>) -> bool {
    n.is_some_and(|s| s.eq_ignore_ascii_case("byblock"))
}

fn usable(p: &[f64]) -> bool {
    p.len() >= 2 && p.iter().all(|v| v.is_finite()) && p.iter().fold(0.0, |s, v| s + v.abs()) > 1e-12 && p.iter().any(|v| *v < 0.0)
}

/// The dash pattern of a linetype in drawing units, or None when it draws continuous.
pub fn pattern_of(doc: &Doc, name: Option<&str>) -> Option<Vec<f64>> {
    if is_cont(name) || is_by_layer(name) || is_by_block(name) {
        return None;
    }
    let name = name.unwrap_or("");
    let up = name.to_uppercase();
    let own = doc.linetypes.get(&name.to_string()).or_else(|| doc.linetypes.values().find(|l| l.name.to_uppercase() == up));
    if let Some(o) = own {
        return usable(&o.pattern).then(|| o.pattern.clone());
    }
    let std = standard(&up)?;
    Some(if hatch_is_metric_doc(None, doc) { std.iter().map(|v| v * 25.4).collect() } else { std })
}

/// The linetype an object draws with: its own, its block's (ByBlock), or its layer's.
pub fn resolve_lt(layers: &Layers, e: &Entity, by_block: Option<&str>, parent_layer: Option<&str>) -> String {
    let mut n: Option<String> = e.lt.clone().flatten();
    if is_by_block(n.as_deref()) {
        n = Some(by_block.unwrap_or("Continuous").to_string());
    }
    if is_by_layer(n.as_deref()) {
        let ln = match (e.layer.as_deref(), parent_layer) {
            (Some("0"), Some(p)) if !p.is_empty() => p,
            (l, _) => l.unwrap_or(""),
        };
        n = Some(
            layers.get(&ln.to_string()).and_then(|l| l.linetype.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| "Continuous".into()),
        );
    }
    n.unwrap_or_else(|| "Continuous".into())
}

/// The lineweight an object draws with (mm; -3 = the default): its own, its block's, or its layer's.
pub fn resolve_lw(layers: &Layers, e: &Entity, by_block: Option<f64>, parent_layer: Option<&str>) -> f64 {
    let mut w: Option<f64> = e.lw.flatten();
    if w == Some(-2.0) {
        w = Some(by_block.unwrap_or(-3.0));
    }
    if w.is_none() || w == Some(-1.0) {
        let ln = match (e.layer.as_deref(), parent_layer) {
            (Some("0"), Some(p)) if !p.is_empty() => p,
            (l, _) => l.unwrap_or(""),
        };
        w = Some(layers.get(&ln.to_string()).and_then(|l| l.lineweight).unwrap_or(-3.0));
    }
    w.filter(|x| x.is_finite()).unwrap_or(-3.0)
}

fn sweep(q: &Prim) -> f64 {
    let mut d = q.a1 - q.a0;
    let mut guard = 0;
    while d <= 0.0 && guard < 1_000_000 {
        d += 2.0 * PI;
        guard += 1;
    }
    d
}
/// The part [f0, f1] (fractions of its length) of one primitive.
fn piece(q: &Prim, f0: f64, f1: f64) -> Prim {
    if q.is_l() {
        return Prim::l(q.x1 + (q.x2 - q.x1) * f0, q.y1 + (q.y2 - q.y1) * f0, q.x1 + (q.x2 - q.x1) * f1, q.y1 + (q.y2 - q.y1) * f1);
    }
    let sw = sweep(q);
    if q.rev() {
        let b0 = q.a1 - sw * f1;
        let b1 = q.a1 - sw * f0;
        return Prim { a0: b0, a1: b1, ..*q };
    }
    Prim { a0: q.a0 + sw * f0, a1: q.a0 + sw * f1, ..*q }
}

fn plen(q: &Prim) -> f64 {
    if q.is_l() { js::hypot(q.x2 - q.x1, q.y2 - q.y1) } else { q.r * sweep(q).abs() }
}

/// About how many pieces [`dash_prims`] makes of `prims` (pattern elements × periods along their length).
pub fn dash_count(prims: &[Prim], pattern: &[f64], scale: f64) -> f64 {
    let period = pattern.iter().fold(0.0, |s, v| s + (v * scale).abs());
    if !(period > 1e-12) {
        return 0.0;
    }
    prims.iter().fold(0.0, |s, q| s + plen(q)) / period * crate::geom::f(pattern.len())
}

/// Dashes of primitives (the pattern runs on along a run of primitives), or the primitives as they are when the
/// pattern is too fine to show.
pub fn dash_prims(prims: &[Prim], pattern: Option<&[f64]>, scale: f64) -> Vec<Prim> {
    let Some(pattern) = pattern else { return prims.to_vec() };
    if !(scale > 0.0) || pattern.is_empty() {
        return prims.to_vec();
    }
    let pp: Vec<f64> = pattern.iter().map(|v| v * scale).collect();
    let period = pp.iter().fold(0.0, |s, v| s + v.abs());
    if !(period > 1e-12) {
        return prims.to_vec();
    }
    let total = prims.iter().fold(0.0, |s, q| s + plen(q));
    if total / period * crate::geom::f(pp.len()) > MAX_DASHES {
        return prims.to_vec();
    }
    let dot = period * 0.01;
    let n = pp.len();
    let mut out = Vec::new();
    let mut i = 0usize;
    let at = |i: usize| pp.get(i).copied().unwrap_or(0.0);
    let mut left = at(0).abs();
    for q in prims {
        let len = plen(q);
        let mut s = 0.0;
        while s < len - 1e-12 {
            let v = at(i);
            let step = js::min(left, len - s);
            if v > 0.0 {
                out.push(piece(q, s / len, (s + step) / len));
            } else if v == 0.0 && left == v.abs() {
                out.push(piece(q, s / len, js::min(len, s + dot) / len));
            }
            s += step;
            left -= step;
            if left <= 1e-12 {
                i = (i + 1) % n;
                left = at(i).abs();
                if left == 0.0 {
                    if at(i) == 0.0 && s < len - 1e-12 {
                        out.push(piece(q, s / len, js::min(len, s + dot) / len));
                    }
                    i = (i + 1) % n;
                    left = at(i).abs();
                }
            }
        }
    }
    out
}
