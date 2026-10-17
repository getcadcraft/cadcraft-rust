//! CadCraft L3: TakeoffCraft's files on cc-takeoff — the estimate as CSV and XLSX (`export.js`, `xlsx.js`), the 3D
//! check view as GLB (`glb.js`), the plan set's PDFs (`pdfset.js`: hashes, pages, titles; pages rendered to ink rasters
//! with `hayro` for auto count), the marked PDF copy (`pdfmarks.js`), the proposal PDF (`bid.js`) and the takeoff
//! attached inside a PDF (`embed.js`), plus the op-script runner behind `cadcraft-cli takeoff`.
//!
//! PDFs written here cannot be byte-identical to pdf-lib's; their parity is semantic: what pdf.js reads back from
//! them (pages, operators, text) equals what it reads back from the JS's (checked by `forks/tools/todiff.mjs`).

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
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::cast_precision_loss)]

pub mod embed;
pub mod export;
pub mod glb;
pub mod pdf;
pub mod pdfmarks;
pub mod pdfread;
pub mod pdfw;
pub mod proposal;
pub mod script;
pub mod stdfont;
pub mod xlsx;
