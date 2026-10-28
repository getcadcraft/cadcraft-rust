//! CadCraft L3: 2D constraints and parameters on a drawing — the Rust form of the CadCraft app's (0.5) gsolve.js (the
//! solver), gcons.js (constraints: making, measuring, solving after edits, files, clipboard), params-doc.js (every
//! parameter / constraint edit as one undo step, over cc-part's params.js) and ckeep.js (MIRROR and TRIM).
//!
//! The drawing keeps the constraints and parameters as data (`Doc::constraints`, `Doc::params`); [`install`] gives
//! cc-cad the hook its `Doc::change` calls before recording a change ([`gcons::after_edit`]), and
//! [`finish_load`] turns a file's parametric payload into constraints after `Doc::load` (gcons.js
//! `resolveParametric`). Every number is computed in the JS's order (fdlibm sin / cos / atan2, V8's hypot), so the
//! solved geometry is the JS's to the bit; kdiff.mjs checks it command by command.

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
// `!(x > y)` is how the JS says "not greater, or NaN": kept as written for parity.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

pub mod ckeep;
pub mod con;
pub mod gcons;
pub mod gsolve;
pub mod paramsdoc;
pub mod swsketch;

pub mod tr {
    /// The language catalog messages come from (cc-cad's).
    pub type Tr = cc_cad::i18n::Catalog;
}

use cc_cad::Doc;
use cc_cad::entity::Entity;

/// The hook `Doc::change` calls: drop constraints whose objects went, re-solve what moved (or refuse the change).
fn edit_hook(doc: &mut Doc, before: &[(u64, Option<Entity>)]) -> Result<Option<String>, cc_cad::DocError> {
    let tr = cc_cad::i18n::catalog();
    gcons::after_edit(doc, &tr, before)
}

/// Give cc-cad the constraint hook (idempotent).
pub fn install() {
    cc_cad::doc::install_edit_hook(edit_hook);
}

/// After `Doc::load`: a file's parametric payload and its entities' CCREF keys → the drawing's constraints and
/// parameters (gcons.js `resolveParametric`), and the notes for the open report.
pub fn finish_load(doc: &mut Doc) -> Vec<String> {
    let Some((raw, refs)) = doc.take_pending_parametric() else { return Vec::new() };
    let tr = cc_cad::i18n::catalog();
    let map: gcons::RefMap = refs.into_iter().map(|(k, id)| (k, id, gcons::model_get(doc, id))).collect();
    let (cs, p, notes) = gcons::resolve_parametric(&tr, &raw, &map);
    doc.constraints = std::sync::Arc::new(cs);
    doc.params = p.to_json();
    doc.load_notes.clone_from(&notes);
    notes
}
