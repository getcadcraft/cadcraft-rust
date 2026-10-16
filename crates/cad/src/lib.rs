//! CadCraft L1: the editable drafting document — the Rust form of the CadCraft app's `doc.js` (0.5).
//!
//! Units and conventions: drawing units (whatever `meta.units` says), Y up, angles in radians. Entities are typed
//! ([`entity`]) and serialise to exactly the JSON of the JS objects; the document ([`doc::Doc`]) keeps the model and
//! each layout's paper space, layers, blocks, named views, text/dimension styles, linetypes, meta and the current
//! style, and records every edit made inside [`doc::Doc::change`] as ops so undo / redo work across spaces exactly as
//! doc.js does (`mark` / `squash` too). [`doc::Doc::load`] takes doc.load's input shape; [`doc::Doc::snapshot`] is the
//! comparison form the parity harness (`forks/tools/gdiff.mjs`) also writes from the JS `Doc`.
//!
//! Left out of M4 (kept only so `load` accepts and keeps what a file brings):
//! - parametric constraints and parameters (gcons.js / params-doc.js): `load`'s `parametric` is kept as given in
//!   `Doc::parametric`; `ccref` keys are dropped from model entities as doc.js does; nothing is solved.
//! - the 3D model (model3d.js): 'face3d' entities and `faces3d` are taken out of the drawing (as split3d does) into
//!   `Doc::faces3d`; CADCRAFT_3D blocks are dropped; nothing is restored.
//! - split screen (panes.js), UCS (ucs.js): `vports` / `ucs` kept as given, not cleaned.
//! - add-on edit hooks and name hooks, the block editor (BEDIT), Studio Live hooks, `swapContent`.
//! - listeners: events queue up instead ([`doc::Doc::drain_events`]).

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

pub mod color;
pub mod doc;
pub mod entity;
pub mod i18n;
pub mod js;
pub mod omap;
pub mod script;

pub use color::{aci_hex, nearest_aci, type_name};
pub use doc::{Block, Doc, DocError, DocEvent, Layer, Layout, MODEL, SpaceId, View};
pub use entity::{Color, Entity, Geo, Obj, Vertex, num, pt};
