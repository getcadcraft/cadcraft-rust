//! cc-ifc in Rust: the IFC reader (port of CadCraft's own `lib/ifc`, Apache-2.0, written from ISO 10303-21:2016 and the
//! IFC2X3 / IFC4 ADD2 TC1 / IFC4.3 specifications): the STEP index and parameter parser ([`step`]), the schema tables
//! ([`schema`]), the open file ([`model`]) and the geometry of products ([`geo`]), at output parity with the 0.5
//! JavaScript (`tools/fork-parity` engines `ifc-step` and `ifc-geom`).
//!
//! Booleans (openings, IfcBooleanResult) go through [`geo::bool_ops::SolidOps`]; [`geo::bool_ops::NoSolidOps`] is the
//! JS path without a boolean engine (half-spaces still cut, convex openings cut out of the triangles).
//!
//! Hostile input: every count from the file is bounded, recursion is bounded by depth counters (references can loop),
//! and nothing here panics on a damaged file.

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
#![deny(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

pub mod engine_geom;
pub mod engine_step;
pub mod geo;
pub mod jsmath;
pub mod model;
pub mod props;
pub mod schema;
pub mod step;
pub mod util;
pub mod value;
pub mod view;

pub use step::{StepError, StepIndex, parse_complex, parse_params, scan_step};
pub use value::{JsStr, Value};
