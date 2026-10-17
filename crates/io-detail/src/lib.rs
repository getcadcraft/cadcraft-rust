//! CadCraft L5: DetailCraft's files — the Rust form of the frozen 0.5 modules `ifc.js` ([`ifc_out`]), `nc1.js`
//! ([`nc1`]), `ifc-import.js` ([`ifc_import`]) and `dwgout.js` ([`dwgout`]), one module per JS file, the same
//! floating-point order, checked against the frozen JS at zero tolerance by `forks/tools/dcdiff.mjs` (the deterministic
//! writers byte for byte). [`script`] holds the call-script ops of `cadcraft-cli detail`.
//!
//! Hostile input: an IFC file goes through cc-ifc's bounded reader; counts are capped (20 000 members), recursion
//! through placements and mapped items is bounded by depth, and nothing here panics on a damaged file.

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
// `!(x > y)` is how the JS says "not greater, or NaN": kept as written for parity.
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::nonminimal_bool)]
#![allow(clippy::cast_precision_loss, clippy::type_complexity, clippy::too_many_arguments, clippy::too_many_lines)]

pub mod dwgout;
pub mod ifc_import;
pub mod ifc_out;
pub mod nc1;
pub mod num;
pub mod script;
