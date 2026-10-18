//! CadCraft L2: the drafting geometry kernel — the Rust form of the CadCraft app's (0.5) geom.js, dims.js,
//! dimstyle.js, units.js, walk.js, font.js, leader.js, linetypes.js and hatch.js (patterns).
//!
//! Each function is ported one for one (same math, same order of floating-point operations, JS `Math.hypot` /
//! `Math.round` / `toFixed` semantics from `cc_cad::js`) and checked against the frozen JS by
//! `forks/tools/gdiff.mjs` (`cadcraft-cli cadgeom`).
//!
//! Units: drawing units, Y up, angles in radians. Primitives ([`geom::Prim`]): `L` segments and `A` arcs (CCW a0 →
//! a1); every entity breaks down into them ([`geom::prims`]).
//!
//! hatch.js `boundaryAt` (HATCH pick-inside) is [`hatch::boundary_at`] since M4.10. Not ported: XCLIP clipping and pictures in walk, add-on grip
//! and name hooks, the per-entity caches (textLayout / dimGeometry / hatchLines are recomputed: pure functions).
//! Where the JS throws on a malformed object (a dimension without its points, a multileader without points), the
//! Rust returns an empty result instead.

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
// flat [x, y, x, y, …] loops read pairwise, as the JS loops do
#![allow(clippy::chunks_exact_to_as_chunks)]

pub mod corners;
pub mod dims;
pub mod dimstyle;
pub mod display;
pub mod edit;
pub mod font;
pub mod geom;
pub mod hatch;
pub mod jsmath;
pub mod leader;
pub mod linetypes;
mod nfd;
pub mod parity;
pub mod pick;
pub mod tx;
pub mod units;
pub mod walk;
