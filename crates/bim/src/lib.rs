//! CadCraft L4: BimCraft's DOM-free engine modules (CadCraft 0.5, `public_html/assets/apps/bimcraft/dev/`), one for
//! one, at exact parity with the frozen JS (`forks/tools/bimdiff.mjs` runs both on the same jobs and compares the
//! canonical JSON, CSV text and BCF bytes at zero tolerance):
//!
//! - [`model`]: ifc-model.js `load()` — spatial tree, storeys, classes, geometry batches with element runs
//!   (on `cc-io-ifc`: the STEP index, the attributes, the ifc-geom port).
//! - [`api`]: the line objects of cc-ifc's IfcAPI (`GetLine`, `toApi`) the modules read.
//! - [`props`]: ifc-props.js — the property index, Color by, colour schemes.
//! - [`find`] (ifc-find.js), [`qto`] (ifc-qto.js), [`checks`] (ifc-checks.js), [`compare`] (ifc-compare.js),
//!   [`clash`] (clash.js; clash-worker.js is only its worker shell), [`bcf`] + [`xml`] (bcf.js),
//!   [`tree`] (ifc-tree.js), [`vis`] (ifc-vis.js), [`elements`] (ifc-elements.js, the reading half),
//!   [`util`] (ifc-util.js matrices).
//! - [`job`]: parity jobs (JSON in, JSON out) for `cadcraft-cli bim`.
//!
//! Everything read from a file or from storage is hostile: no panics, counts and depths capped, the JS's own caps
//! kept (BCF zips: 20 MB per file, 200 MB total, 5,000 entries, 64 levels, 2,000 topics). No UI toolkit, no clock.

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
#![allow(clippy::neg_cmp_op_on_partial_ord)]

pub mod api;
pub mod bcf;
pub mod checks;
pub mod clash;
pub mod compare;
pub mod digest;
pub mod elements;
pub mod find;
pub mod job;
pub mod js;
pub mod model;
pub mod props;
pub mod qto;
pub mod tree;
pub mod util;
pub mod vis;
pub mod xml;
