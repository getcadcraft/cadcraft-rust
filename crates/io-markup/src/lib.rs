//! CadCraft L3: MarkupCraft's PDF side — the Rust form of `pdf-read.js` (open a plan set, read its annotations back
//! as markups, threads, calibrations, sheet names, layers, bookmarks; sheet numbers and titles from the page text) and
//! `pdf-write.js` (save a marked-up copy: every markup a real annotation with an appearance stream drawn from the same
//! display list as the screen, the private `/MarkupCraft` JSON, reply annotations, /VP calibrations, /PageLabels,
//! optional content groups, the outline).
//!
//! The JS uses two libraries; so does this crate: the reading side sees the file as pdf.js does (cross-reference
//! tables and streams, object streams, repair, decryption — through `hayro-syntax`; [`jsdoc`], [`annots`], [`text`]),
//! and the writing side and the "extras" as pdf-lib does (every object in file order — [`libdoc`], [`obj`]). Writing
//! cannot be byte-identical to pdf-lib; the parity target is what the frozen pdf.js and pdf-lib read back
//! (`forks/tools/mkdiff.mjs --pdf`).
//!
//! Everything read is hostile: object counts, nesting depth, inflated sizes, page-tree depth, text-extraction work and
//! image sizes are capped; nothing panics.

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
#![allow(clippy::neg_cmp_op_on_partial_ord)]
#![allow(clippy::cast_precision_loss)]

pub mod annots;
pub mod image;
pub mod jsdoc;
pub mod libdoc;
pub mod obj;
pub mod ops;
pub mod pages;
pub mod picture;
pub mod read;
pub mod summary;
pub mod text;
pub mod write;

pub const PRIVATE_KEY: &str = "MarkupCraft";
pub const PRIVATE_CAL_KEY: &str = "MarkupCraftCal";
pub const PRIVATE_SHEET_KEY: &str = "MarkupCraftSheet";
pub const PRIVATE_PHOTO_KEY: &str = "MarkupCraftPhoto";
pub const PRIVATE_COLUMNS_KEY: &str = "MarkupCraftColumns";
pub const PRIVATE_LINK_KEY: &str = "MarkupCraftLink";
pub const PRIVATE_LAYERS_KEY: &str = "MarkupCraftLayers";
pub const BLANK_KEY: &str = "MarkupCraftBlank";

/// `refKey(objectNumber, generation)`: "12R" / "12R3" (pdf.js annotation ids).
pub fn refkey(n: u32, g: u16) -> String {
    if g == 0 { format!("{n}R") } else { format!("{n}R{g}") }
}
