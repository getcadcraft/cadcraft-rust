//! cc-skp in Rust: the SketchUp (.skp) reader, a port of CadCraft's own `lib/skp` (Apache-2.0) at exact output
//! parity. Format knowledge comes from OpenSKP (MIT, read as a map by the JS authors; see `NOTICE`); this code is ours.
//!
//! `read_skp(bytes)` → [`Skp`], the model of `readSkp(bytes)` in `lib/skp/read.js` (docs/SKP-LIB-SPEC.md): materials,
//! tags, definitions (faces with holes, loose edges, soft edges, instances), thumbnail, display unit, notes, counts.
//! Coordinates are inches, Z up. A file it refuses comes back as [`SkpError`] with the JS `code` and message.
//! It never panics on hostile input: offsets and lengths are checked, ZIP entries are capped (size, ratio, total),
//! object nesting is depth-capped, and the container walks are iterative.

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

pub mod legacy;
pub mod messages;
pub mod model;
pub mod read;
pub mod util;
pub mod vff;
pub mod zip;

pub use model::{Container, Counts, Def, Edge, Face, Image, Inst, Layer, Material, Skp, Texture};
pub use read::{Sniff, read_skp, sniff_skp};
pub use util::{LIMITS, Limits, SkpError};
