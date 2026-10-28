//! CadCraft L3: CadCraft's LISP — the 0.5 app's `lisp/` folder (reader.js, interp.js, builtins.js, cadfns.js, vl.js,
//! vla.js, dcl.js, wild.js, missing.js) in Rust. The drawing side (host-cad.js, vla-host.js, dxflist.js) is the
//! engine's ([`host::Host`]); this crate has no drawing of its own.
//!
//! Written from 0.5's clean-room interpreter (itself written from the public AutoLISP reference), function by function,
//! with its JS semantics where they show: integers are 32-bit and wrap (`| 0`, `Math.imul`), strings are UTF-16,
//! reals print with six significant digits, V8's Math functions. Checked against the frozen JS by
//! `forks/tools/lispdiff.mjs`.
//!
//! The evaluator is a machine with its frames on the heap ([`interp`]): a routine recursing a million deep needs memory,
//! not stack; it stops at its step budget, its frame cap, or Esc. The drawing side's questions (getpoint, a running
//! (command …)) are awaited at the machine's top level, so the whole interpreter runs inside an engine command's
//! future on the window's thread (desktop and web alike).

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
// `!(x > y)` is how the JS says "not greater, or NaN": kept as written for parity.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

pub mod builtins;
pub mod cadfns;
pub mod dcl;
pub mod host;
pub mod interp;
pub mod missing;
pub mod reader;
pub mod syms;
pub mod value;
pub mod vl;
pub mod vla;
pub mod wild;

use std::rc::Rc;

pub use host::Host;
pub use interp::{Limits, Lisp, Outcome};
pub use value::{LErr, V};

/// One ready interpreter with every function installed (runtime.js `createLisp`).
pub fn new_lisp(host: Option<Rc<dyn Host>>, out: Rc<dyn Fn(&str)>) -> Lisp {
    let mut l = Lisp::bare(host, out);
    builtins::install(&mut l);
    cadfns::install(&mut l);
    vl::install(&mut l);
    vla::install(&mut l);
    dcl::install(&mut l);
    l
}

/// Run a future that never waits (no drawing side, or one that answers at once): its output, or None when it waits.
pub fn block_on<F: std::future::Future>(f: F) -> Option<F::Output> {
    let mut f = std::pin::pin!(f);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    match f.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(v) => Some(v),
        std::task::Poll::Pending => None,
    }
}
