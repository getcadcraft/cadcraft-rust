//! CadCraft L3: Studio Live, the protocol (docs/LIVE-PROTOCOL.md version 1 of the CadCraft site; AGENTS.md § 15).
//!
//! - [`msgpack`]: the wire values, as live-client.js packs and unpacks them (bounded, never panics on input).
//! - [`envelope`]: `{t, s, c, n, q, b}`.
//! - [`session`]: the session core — roles, locks, holds and the rules that decide each edit from its `touched` keys,
//!   as Studio's session server does (the shared vectors' scenarios pass on it).
//! - [`client`]: a client core usable without a screen: numbered pending edits as op records, commits applied in order,
//!   key versions learned, refusals, rejoin (`hello {resume}`).
//!
//! Not here yet: the network (WebSocket to a host or the relay), the PAKE handshake and sealed frames (LIVE-CRYPTO.md),
//! snapshots and the host handoff. Tests never connect anywhere.

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

pub mod client;
pub mod envelope;
pub mod msgpack;
pub mod session;
