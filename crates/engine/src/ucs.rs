//! ucs.js, the part the command line needs: the UCS in force (typed x,y and d<a are in it; new text runs along it).
//! A UCS is 2D: an origin and the angle of its X axis; `None` is the world.

use cc_cad::Doc;
use cc_cad::js;
use cc_cad_geom::geom::{TAU, norm_angle};
use cc_cad_geom::jsmath::JsMath;
use cc_geom::P2;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ucs {
    pub x: f64,
    pub y: f64,
    pub ang: f64,
}

fn is_world(u: &Ucs) -> bool {
    let a = norm_angle(u.ang);
    u.x.abs() < 1e-12 && u.y.abs() < 1e-12 && js::min(a, TAU - a) < 1e-12
}

/// `cleanUcs(v)`: untrusted data → a UCS, or None for the world / anything unusable.
pub fn clean_ucs(v: Option<&Value>) -> Option<Ucs> {
    let v = v.filter(|v| v.is_object() || v.is_array())?;
    let x = js::to_number_opt(v.get("x"));
    let y = js::to_number_opt(v.get("y"));
    let ang = js::to_number_opt(v.get("ang"));
    if ![x, y, ang].iter().all(|n| n.is_finite()) || x.abs() > 1e15 || y.abs() > 1e15 {
        return None;
    }
    let u = Ucs { x, y, ang: norm_angle(ang) };
    if is_world(&u) { None } else { Some(u) }
}

/// `activeUcs(doc)`: the current UCS in the model (sheets are paper coordinates). doc.js keeps `ucs.current`
/// cleaned at load; the Rust Doc keeps the file's `{ current, named }` as given, so it is cleaned here.
pub fn active_ucs(doc: &Doc) -> Option<Ucs> {
    if !doc.in_model() {
        return None;
    }
    clean_ucs(doc.ucs.as_ref().and_then(|u| u.get("current")))
}

pub fn vec_to_world(u: Option<Ucs>, v: P2) -> P2 {
    let Some(u) = u else { return v };
    let (c, s) = (u.ang.jcos(), u.ang.jsin());
    P2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

pub fn to_world(u: Option<Ucs>, q: P2) -> P2 {
    let Some(uu) = u else { return q };
    let w = vec_to_world(u, q);
    P2::new(uu.x + w.x, uu.y + w.y)
}

/// The UCS angle (0 for the world).
pub fn ang_of(u: Option<Ucs>) -> f64 {
    u.map_or(0.0, |u| u.ang)
}
