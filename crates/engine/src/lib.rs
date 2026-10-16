//! CadCraft L5: the session's command engine (port of the CadCraft app's commands.js, in progress — M4).
//!
//! [`prompt`] is the question machinery the commands are written in: `async` command bodies awaiting answers,
//! driven by [`prompt::Runner`] without a runtime (desktop and wasm alike).

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

pub mod prompt;
