//! CadCraft L3: DetailCraft's engine — the Rust form of the DOM-free modules of DetailCraft 0.5
//! (`public_html/assets/apps/detailcraft/dev/`), one module per JS file, the same floating-point order, checked
//! against the frozen JS at zero tolerance by `forks/tools/dcdiff.mjs` (`cadcraft-cli detail <script.json>`).
//!
//! Millimetres throughout, z up. The model's state is plain JSON (as in the JS), so the untrusted-file checks
//! ([`model::read_model`]), the undo snapshots and the saved file are the JS's own, byte for byte; the derived
//! modules read it through typed views ([`state`]).
//!
//! - [`sections`] (the shared section table) + [`profiles`], [`geom`], [`model`], [`connections`], [`build`],
//!   [`marks`], [`rebar`], [`stair`], [`csv`], [`zip`], [`sample`] …
//! - [`script`]: the call-script runner behind `cadcraft-cli detail` and the parity harness.

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
#![deny(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]
// `!(x > y)` is how the JS says "not greater, or NaN": kept as written for parity.
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::nonminimal_bool)]
// counts become JS numbers (f64) by design
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::type_complexity, clippy::too_many_arguments, clippy::too_many_lines)]

pub mod build;
pub mod connections;
pub mod csv;
pub mod geom;
pub mod i18n;
pub mod js;
pub mod marks;
pub mod model;
pub mod profiles;
pub mod rebar;
pub mod sample;
pub mod script;
mod section_rows;
pub mod sections;
pub mod stair;
pub mod state;
pub mod zip;
