//! CadCraft L2: CivilCraft's engine — the Rust form of the DOM-free modules of CivilCraft 0.5
//! (`public_html/assets/apps/civilcraft/dev/`), one module per JS file, the same floating-point order, checked
//! against the frozen JS at zero tolerance by `forks/tools/cvdiff.mjs` (`cadcraft-cli civil <script.json>`).
//!
//! Units: a project is in US survey feet or metres; every coordinate, elevation and length is in that unit; x =
//! easting, y = northing, z = elevation. Large (survey) coordinates are handled the way the JS handles them: a TIN
//! works in local coordinates relative to its floored minimum corner ([`tin`]), so state-plane and UTM values keep
//! their precision.
//!
//! - [`units`], [`points`], [`tin`] (+ [`delaunator`], [`predicates`]), [`alignment`], [`profile`] …
//! - [`script`]: the call-script runner behind `cadcraft-cli civil` and the parity harness.
//!
//! Where the JS relies on the engine (a typed array read at NaN, V8's sort on an inconsistent comparator) the
//! difference is written next to the code as a "Spec gap".

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
// the JS's nested shapes (closures over typed parts) are kept as they are
#![allow(clippy::type_complexity)]

pub mod alignment;
pub mod civil_agent;
pub mod clip;
pub mod corridor;
pub mod delaunator;
pub mod desckeys;
pub mod drive;
pub mod featureline;
pub mod geo;
pub mod gis_state;
pub mod grading;
pub mod hydro;
pub mod i18n;
pub mod js;
pub mod masshaul;
pub mod parcels;
pub mod pipes;
pub mod point_edit;
pub mod points;
pub mod predicates;
pub mod profile;
pub mod project;
pub mod script;
pub mod script_ops;
pub mod script_ops2;
pub mod sections;
pub mod site3d;
pub mod solve_why;
pub mod staking;
pub mod superelev;
pub mod tin;
pub mod traverse;
pub mod units;
