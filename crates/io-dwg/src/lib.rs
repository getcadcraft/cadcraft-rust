//! cc-dwg in Rust: the DWG reader (port of CadCraft's own clean-room `lib/dwg`, Apache-2.0, written from the
//! ODA "Open Design Specification for .dwg files" v5.4.1). R13–R2018 (AC1012–AC1032).
//!
//! `read_dwg(bytes, &ReadOpts::default())` → the model as a [`value::Obj`] with exactly the shape of the
//! JavaScript `readDwg` (lib/dwg/model.js), so `convert --json` can be diffed against it field for field.
//! It never panics on a damaged file: what cannot be read is noted in `report.notes`.

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

pub mod acds;
pub mod acis;
pub mod bits;
pub mod container;
pub mod crc;
pub mod dec_entities;
pub mod dec_mleader;
pub mod dec_objects;
pub mod dec_objects2;
pub mod dynblock_read;
pub mod header_vars;
pub mod lz2004;
pub mod messages;
pub mod mleader_read;
pub mod model;
pub mod objects;
pub mod passthrough;
pub mod r2007;
pub mod read;
pub mod solid_read;
pub mod surface_read;
pub mod table_read;
pub mod util;
pub mod value;

pub use read::{ReadOpts, read_dwg};
pub use value::{Obj, V};
