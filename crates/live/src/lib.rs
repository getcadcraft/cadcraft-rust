//! CadCraft L3: Studio Live, the protocol (docs/LIVE-PROTOCOL.md version 1 of the CadCraft site; AGENTS.md § 15).
//!
//! - [`msgpack`]: the wire values, as live-client.js packs and unpacks them (bounded, never panics on input).
//! - [`envelope`]: `{t, s, c, n, q, b}`.
//! - [`session`]: the session core — roles, locks, holds and the rules that decide each edit from its `touched` keys,
//!   as Studio's session server does (the shared vectors' scenarios pass on it).
//! - [`client`]: a client core usable without a screen: numbered pending edits as op records, commits applied in order,
//!   key versions learned, refusals, rejoin (`hello {resume}`).
//! - [`crypto`]: the PAKE handshake and sealed frames (LIVE-CRYPTO.md), byte for byte 0.5's crypto.rs and live-crypto.js.
//! - [`server`] (desktop): the session server's whole core as 0.5 Studio's `live/session.rs` — joining with codes and
//!   tickets, approval, catch-up from the log or a snapshot, holds, locks, presence, pings, the replica, assistants,
//!   the host handoff's state.
//! - [`ws`], [`net`] (desktop): a small RFC 6455 WebSocket on std::net, the server's transport on threads and a
//!   joiner's connection. LAN `ws://` only: the relay (wss://) is the web edition's.
//!
//! Tests run the shared vectors on the core, over loopback sockets (plain and sealed), and against the 0.5 JS client in
//! the fork's harness.

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
pub mod crypto;
pub mod envelope;
pub mod msgpack;
#[cfg(not(target_arch = "wasm32"))]
pub mod net;
#[cfg(not(target_arch = "wasm32"))]
pub mod server;
pub mod session;
#[cfg(not(target_arch = "wasm32"))]
pub mod ws;
