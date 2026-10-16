//! CadCraft L1: the 3D model kernel — the Rust form of ModelCraft's (0.5) `model.js` with `vec.js`, `earcut.js` and
//! the words of `i18n.js` it uses.
//!
//! Units and conventions: metres, Z up, X red, Y green, Z blue; matrices are 16 numbers column-major; ids are JS
//! numbers handed out by counters exactly as the JS hands them out (so `f64`); maps keep JS `Map` insertion order
//! ([`omap`]). Every function is ported one for one with the same order of floating-point operations, V8's
//! `Math.hypot` / `Math.round` / `Math.sin` / `Math.cos` ([`js`], [`jsmath`]) and `JSON.stringify`'s number text
//! ([`json`]), and is checked against the frozen JS by `forks/tools/mdiff.mjs` (`cadcraft-cli m3d`) at zero tolerance.
//!
//! - [`def`]: definitions (vertices, edges, faces, placements, smooth-curve points) and their basic queries.
//! - [`edges`]: addEdge / reface / cleanup / soften / smooth normals — the sticky geometry.
//! - [`ops`]: push/pull, erase, move/rotate/scale/flip, copy, shapes, curves, smooth curves, hide, follow me.
//! - [`offset`], [`paint`], [`solids`], [`mesh`] (meshDef / edgesDef / worldTriangles / intersectFaces).
//! - [`model`]: the Model — undo / redo / amend / previews over JSON snapshots, groups and components, tags,
//!   sections, axes, scenes, lamps, guides, clipboard; [`dims`]: dimensions; [`file`]: toJSON / load.
//! - [`script`]: the op-script runner behind `cadcraft-cli m3d` and the parity harness.
//!
//! Where the JS would throw a TypeError on input it does not expect, the port returns [`error::Error::Type`]; where
//! the JS relies on the engine (V8's sort, its stack depth, `Date.now` in a session id) the difference is written next
//! to the code as a "Spec gap".

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]
// `!(x > y)` is how the JS says "not greater, or NaN": kept as written for parity.
#![allow(clippy::neg_cmp_op_on_partial_ord)]
// ids and counts are JS numbers (f64) by design
#![allow(clippy::cast_precision_loss)]

pub mod def;
pub mod dense;
pub mod dims;
pub mod earcut;
pub mod edges;
pub mod error;
pub mod file;
pub mod i18n;
pub mod js;
pub mod jsmath;
pub mod json;
pub mod mesh;
pub mod meshin;
pub mod model;
pub mod offset;
pub mod omap;
pub mod ops;
pub mod paint;
pub mod part;
pub mod script;
pub mod solids;
pub mod sort;
pub mod units;
pub mod vec;

pub use def::Def;
pub use error::{Error, Res};
pub use model::Model;
pub use ops::Sel;
