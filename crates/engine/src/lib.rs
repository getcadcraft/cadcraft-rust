//! CadCraft L5: the session's command engine — the CadCraft app's (0.5) commands.js command line, M4.
//!
//! [`prompt`] is the question machinery the commands are written in: `async` command bodies awaiting answers,
//! driven by [`prompt::Runner`] without a runtime (desktop and wasm alike). [`engine::Engine`] is the command line
//! (typed text, clicks, selection windows, Esc; repeat-last on Enter; one undo step per command) over a
//! [`engine::Host`]; [`cmd`] holds the drafting commands ported so far: LINE PLINE CIRCLE (2P 3P TTR TTT) TTR ARC
//! RECTANG POLYGON ELLIPSE POINT TEXT ERASE MOVE COPY ROTATE SCALE MIRROR OFFSET TRIM EXTEND UNDO REDO ZOOM.
//! Parity with the frozen JS is checked by `forks/tools/kdiff.mjs` (`cadcraft-cli cmds`).
//!
//! Not ported here: constraints carried through MIRROR / TRIM (ckeep.js; the M4 Doc solves no constraints), the 3D
//! tab's versions of MOVE / COPY / ROTATE / ERASE / ZOOM, lasso selection, STRETCH's window memory.

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
#![deny(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

pub mod cmd;
pub mod engine;
pub mod input;
pub mod lang;
pub mod prompt;
pub mod tancircle;
pub mod ucs;

pub use engine::{Ctx, Engine, Host, IdSet, Opts, Reply, Tan};
pub use lang::Lang;
