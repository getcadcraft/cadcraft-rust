//! CadCraft L2: MarkupCraft's engine — the Rust form of the DOM-free modules of MarkupCraft 0.5
//! (`assets/apps/markupcraft/dev/`): `model.js` (the [`model::Store`] with undo / redo, makeMarkup, threads),
//! `shapes.js` (one display list for screen and PDF), `geom.js` (grips, moving, picking, alignment), `font.js`
//! (Helvetica metrics, wrapping, WinAnsi), `units.js` (lengths, scales, areas), `arcs.js`, `holes.js`,
//! `columns.js`, `imagestamp.js`, `titleblock.js` (sheet number and title from page text), `chest.js` (tool chest),
//! `clip.js` (clipboard), `csv.js`, `bcf.js` (BCF 2.1 zip), `bcf-read.js` (BCF import), `compare.js` (sheet-compare
//! mixing), `xlsx.js` + `summary.js` (the workbook summary), `slip.js` (slip-sheeting), `link.js` (sheet links and
//! callouts), `spaces.js` / `sketch.js` / `legend.js` ([`spaces`]), `textsearch.js` (find text), `vecdiff.js` (compare
//! by vector paths) and the DOM-free halves of `pages.js`, `blank.js` and `bookmarks.js` ([`pages`]), with the words of
//! `i18n.js` over the 0.5 English catalog and addons.js `cleanAddonData`.
//!
//! Units: PDF user space (points, y up). Every function is ported one for one with the same order of floating-point
//! operations and V8's `Math` / number formatting (through `cc-cad::js` and `cc-m3d::js` / `jsmath`), and is checked
//! against the frozen JS by `forks/tools/mkdiff.mjs` (`cadcraft-cli markup`) at zero tolerance.
//!
//! Time and ids never come from the system inside the engine: a [`model::Env`] (the clock, the id counter) is
//! passed in, so a session replays exactly. Untrusted input (clips, tool chests, private data in files) is checked
//! field by field with the JS's caps (2 000 clip items, 500 tools, 100 000 points, ±10^7 coordinates, 200 holes).

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
// counts are JS numbers (f64) by design
#![allow(clippy::cast_precision_loss)]

pub mod addons;
pub mod arcs;
pub mod bcf;
pub mod bcfread;
pub mod chest;
pub mod clip;
pub mod collate;
pub mod columns;
pub mod compare;
pub mod csv;
pub mod en;
pub mod font;
pub mod geom;
pub mod holes;
pub mod i18n;
pub mod imagestamp;
pub mod js;
pub mod json;
pub mod link;
pub mod model;
pub mod nfd;
pub mod pages;
pub mod script;
pub mod script2;
pub mod shapes;
pub mod slip;
pub mod spaces;
pub mod summary;
pub mod textsearch;
pub mod titleblock;
pub mod units;
pub mod vecdiff;
pub mod xlsx;

pub use model::{Env, Markup, Store};
