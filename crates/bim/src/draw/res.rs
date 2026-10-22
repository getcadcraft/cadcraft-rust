//! res-follow.js (0.5 part 4, track RC): what of the residential set follows the walls. `syncDerived` (model-slabs.js)
//! calls [`sync_residential`] at its end — after every wall change and every level change — and the door / window
//! changes call it for their walls' levels, so the terrain's pads (once the ground is loaded), foundations,
//! countertops, stairs, electrical devices and framing are made again from the walls in the same undo step.
//!
//! The parts each kind is made of live in [`crate::draw::resfound`], [`crate::draw::rescounter`],
//! [`crate::draw::resstair`], [`crate::draw::reselec`], [`crate::draw::resframe`] and [`crate::draw::resterrain`];
//! this module holds what they share.

use std::collections::HashMap;

use cc_io_ifc::model::Id;

use crate::api::R;
use crate::draw::levels::{Level, read_levels};
use crate::draw::m::{V2, at, cross, hypot, nx, pv, sub};
use crate::draw::slabs::StoreyLoops;
use crate::draw::writer::W;
use crate::wapi::P;

/// What the level's sync already read (`syncDerived`'s `{ levels, L }`).
pub struct ResCtx<'c> {
    pub levels: Option<&'c [Level]>,
    pub l: Option<&'c StoreyLoops>,
}

/// `syncResidential(w, storey, ctx)` → how many elements were made again.
pub fn sync_residential(w: &mut W<'_>, storey: Id, ctx: &ResCtx<'_>) -> R<usize> {
    let mut n = 0usize;
    // first: the building pad round the walls (and the grade, the paving) — footings step to it
    n = n.saturating_add(crate::draw::resterrain::sync_terrain(w, storey, ctx.levels)?);
    n = n.saturating_add(crate::draw::resfound::sync_foundations(w, storey, ctx)?);
    n = n.saturating_add(crate::draw::rescounter::sync_countertops(w, storey)?);
    n = n.saturating_add(crate::draw::resstair::sync_stairs(w, storey, ctx.levels)?);
    // outlets, switches and panels stay on their walls
    n = n.saturating_add(crate::draw::reselec::sync_electrical(w, storey)?);
    // last: walls, floors, roofs and their openings are final by now
    n = n.saturating_add(crate::draw::resframe::sync_framing(w, storey)?);
    Ok(n)
}

/// The levels from the context, else read now.
pub fn levels_or(w: &mut W<'_>, levels: Option<&[Level]>) -> R<Vec<Level>> {
    match levels {
        Some(l) => Ok(l.to_vec()),
        None => read_levels(w),
    }
}

/// `partsOf(w, id)`: what the first IfcRelAggregates relating `id` aggregates.
pub fn parts_of(w: &W<'_>, id: Id) -> R<Vec<Id>> {
    for rid in w.ids("IFCRELAGGREGATES") {
        let r = w.must(rid)?;
        if r.a("RelatingObject").href() == Some(id) {
            return Ok(r.a("RelatedObjects").hrefs());
        }
    }
    Ok(Vec::new())
}

/// The first IfcRelAggregates relating `id`.
pub fn agg_rel(w: &W<'_>, id: Id) -> R<Option<Id>> {
    for rid in w.ids("IFCRELAGGREGATES") {
        if w.must(rid)?.a("RelatingObject").href() == Some(id) {
            return Ok(Some(rid));
        }
    }
    Ok(None)
}

/// `insetPoly(P, d)`: an anticlockwise polygon moved inward by d (outward when d < 0), corners mitred.
pub fn inset_poly(p: &[V2], d: f64) -> Vec<V2> {
    let n = p.len();
    struct E {
        a: V2,
        u: V2,
        n: V2,
    }
    let e: Vec<E> = (0..n)
        .map(|i| {
            let (a, b) = (at(p, i), at(p, nx(i, n)));
            let l = hypot(b[0] - a[0], b[1] - a[1]);
            let l = if l != 0.0 && !l.is_nan() { l } else { 1.0 };
            let u = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
            E { a, u, n: [-u[1], u[0]] }
        })
        .collect();
    (0..n)
        .filter_map(|i| {
            let (ei, f) = (e.get(i)?, e.get(pv(i, n))?);
            let p0 = [f.a[0] + f.n[0] * d, f.a[1] + f.n[1] * d];
            let p1 = [ei.a[0] + ei.n[0] * d, ei.a[1] + ei.n[1] * d];
            let c = cross(f.u, ei.u);
            if c.abs() < 1e-9 {
                return Some(p1);
            }
            let t = cross(sub(p1, p0), ei.u) / c;
            Some([p0[0] + f.u[0] * t, p0[1] + f.u[1] * t])
        })
        .collect()
}

/// `v === true || /^(\.?T\.?|true)$/i.test(String(v ?? ''))`
pub fn is_true(v: Option<&P>) -> bool {
    match v {
        Some(P::Bool(true)) => true,
        None | Some(P::Undef | P::Null) => false,
        Some(x) => {
            let s = x.string();
            let u = s.to_ascii_uppercase();
            matches!(u.as_str(), "T" | ".T" | "T." | ".T." | "TRUE")
        }
    }
}

/// `v === false || /^(\.?F\.?|false)$/i.test(String(v ?? ''))`
pub fn is_false(v: Option<&P>) -> bool {
    match v {
        Some(P::Bool(false)) => true,
        None | Some(P::Undef | P::Null) => false,
        Some(x) => {
            let s = x.string();
            let u = s.to_ascii_uppercase();
            matches!(u.as_str(), "F" | ".F" | "F." | ".F." | "FALSE")
        }
    }
}

/// A pset as read (`w.readPset`).
pub type Ps = HashMap<String, P>;

/// `ps[k] || ''` as text
pub fn s_or(ps: &Ps, k: &str) -> String {
    crate::draw::walls::ps_str(ps, k)
}
/// `Number(ps[k])`
pub fn num(ps: &Ps, k: &str) -> f64 {
    crate::draw::walls::ps_num(ps, k)
}
/// `Number(ps[k] || 0)`
pub fn num0(ps: &Ps, k: &str) -> f64 {
    crate::draw::walls::ps_num0(ps, k)
}
/// `ps[k] === undefined` (a missing property)
pub fn missing(ps: &Ps, k: &str) -> bool {
    matches!(ps.get(k), None | Some(P::Undef))
}
