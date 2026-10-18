//! CadCraft L6: tools for AI assistants and scripts (docs/AGENTS-SPEC.md of the 0.5 site; AGENTS.md § 9, § 15).
//!
//! The apps contain no AI: this gives the person's own assistant tools, and the person decides which permission groups
//! (read, edit, save, export) are allowed. Tool names, titles, descriptions and argument schemas are 0.5's, so an
//! assistant set up for CadCraft 0.5 works unchanged.
//!
//! - [`rpc`]: the dispatcher (tools/mcp/core.mjs): JSON-RPC 2.0, MCP + the control channel's methods, argument checks
//!   ([`schema`]), permissions, the call log.
//! - [`http`] (desktop): the local control channel with Studio's rules (desktop-shell control.rs): 127.0.0.1, bearer
//!   token ([`token`]), Host check, Origin refused, JSON only, 4 MB.
//! - [`confine`] (desktop): files only inside the named folders.

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

pub mod cad;
pub mod defs;
pub mod ifc;
pub mod json;
pub mod live;
pub mod markups;
pub mod modelcraft;
pub mod rpc;
pub mod schema;
pub mod takeoff;

#[cfg(not(target_arch = "wasm32"))]
pub mod confine;
#[cfg(not(target_arch = "wasm32"))]
pub mod headless;
#[cfg(not(target_arch = "wasm32"))]
pub mod http;
#[cfg(not(target_arch = "wasm32"))]
pub mod token;

pub use rpc::{CallCtx, Dispatcher, RpcError, Session, ToolDef, ToolError, ToolOut, Tools};
