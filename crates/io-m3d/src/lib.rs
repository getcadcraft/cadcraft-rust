//! CadCraft L3: ModelCraft's (0.5) file formats on the cc-m3d kernel — the Rust form of `io.js` (OBJ + MTL, STL, GLB /
//! glTF, Collada, DXF / DWG site plans in and DXF / DWG out, IFC4 out), `xml.js` (the strict XML reader Collada uses),
//! `strokefont.js` (the single-stroke font drawn texts become), `solids.js` (Union / Subtract / Intersect through
//! cc-manifold), `skp-import.js` (a cc-skp model → definitions), `print3d.js` (printer readiness, binary STL, 3MF) and
//! `print-mc.js` (what ModelCraft hands it), plus the native `.modelcraft` file (`Model::to_json_string` + images).
//!
//! Every function is ported one for one from the frozen 0.5 JavaScript with the same order of floating-point
//! operations, V8's number formatting ([`num`]) and `Math` functions, and is checked against it by
//! `forks/tools/iodiff.mjs` (`cadcraft-cli m3dio`): byte for byte for the writers, at zero tolerance on the model JSON
//! for the readers.
//!
//! Layering: this crate is layer 3 and so may not use cc-io-dwg or cc-io-skp (also layer 3). The DWG importer takes
//! the cc-dwg drawing model as JSON (`readDwg`'s shape, [`site::import_dwg`]), the DWG exporter returns the cc-dwg
//! model as JSON ([`cad::to_dwg_model`]) for cc-io-dwg's writer, and the SketchUp importer takes its own copy of the
//! cc-skp model shape ([`skp::SkpModel`]); `cadcraft-cli` converts between them.
//!
//! Every reader treats its input as hostile: sizes and counts are capped from the input (the JS's own limits:
//! 2 million vertices / faces / line pieces, 5 million XML elements, 200 000 node visits), offsets are checked,
//! recursion is bounded (glTF and Collada nodes 64 deep, blocks 16 deep, definitions 32 deep), and nothing panics.

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
// ids and counts are JS numbers (f64) by design
#![allow(clippy::cast_precision_loss)]

pub mod cad;
pub mod collada;
pub mod gltf;
pub mod i18n;
pub mod ifc;
pub mod jsx;
pub mod native;
pub mod num;
pub mod obj;
pub mod print3d;
pub mod printmc;
pub mod site;
pub mod skp;
pub mod solids;
pub mod stl;
pub mod strokefont;
pub mod util;
pub mod xml;

pub use cc_m3d::{Error, Res};
