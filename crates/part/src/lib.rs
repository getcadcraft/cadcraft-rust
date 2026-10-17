//! CadCraft L2: PartCraft's engine — the Rust form of the DOM-free modules of PartCraft 0.5
//! (`public_html/assets/apps/partcraft/dev/`), one module per JS file, the same floating-point order, checked against
//! the frozen JS at zero tolerance by `forks/tools/pcdiff.mjs` (`cadcraft-cli part <script.json>`).
//!
//! Units are millimetres; the world is Y up; base planes front = XY (n +Z), top = XZ (n +Y, sketch y = −Z), right = YZ
//! (n +X, sketch x = −Z). The part document is plain JSON (insertion-ordered `serde_json::Value`), as in the JS, so a
//! file read and written back keeps its key order and `toJSON` gives the JS's text byte for byte.
//!
//! - [`part`] (part.js) with [`readers`] (design-features.js, bodies.js, surface.js, sheetmetal.js, rib.js and
//!   threads.js file readers), [`form`] (the form cage reader), [`gdt`], [`bom`], [`configs`];
//! - [`expr`], [`params`], [`params_part`]: the expression language, the parametric engine, the part adapter;
//! - [`sketch`], [`spline`], [`solver`]: the sketcher;
//! - [`threads`]: thread tables, profiles and callouts; [`font`]: Helvetica metrics.
//!
//! Where the JS relies on the engine (V8's TypeError texts, `Object.prototype` names in plain-object tables) the
//! behaviour is reproduced where a file can reach it, and otherwise written next to the code as a "Spec gap".

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
// counts become JS numbers (f64) by design
#![allow(clippy::cast_precision_loss)]
// the JS's shapes are kept (closures over the document, many small arguments)
#![allow(clippy::type_complexity, clippy::too_many_arguments)]

pub mod analysis;
pub mod asm;
pub mod asm_drawing;
pub mod bom;
pub mod clipboard;
pub mod configs;
pub mod drawing;
pub mod explode;
pub mod export;
pub mod expr;
pub mod faces;
pub mod fdlibm;
pub mod font;
pub mod form;
pub mod gdt;
pub mod i18n;
pub mod ifc_export;
pub mod js;
pub mod jv;
pub mod measure;
pub mod meshfaces;
pub mod omap;
pub mod params;
pub mod params_part;
pub mod params_uses;
pub mod part;
pub mod pcscript;
pub mod re;
pub mod readers;
pub mod refgeom;
pub mod script;
pub mod script2;
pub mod script3;
pub mod script4;
pub mod script5;
pub mod shapes_parts;
pub mod sketch;
pub mod sketch_intersect;
pub mod sketch_tools;
pub mod solver;
pub mod spline;
pub mod stdparts;
pub mod threads;
pub mod v8json;
pub mod vec;
