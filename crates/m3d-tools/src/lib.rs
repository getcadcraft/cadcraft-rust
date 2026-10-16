//! CadCraft L4: ModelCraft's interaction layer over the cc-m3d kernel — the Rust form of ModelCraft's (0.5)
//! `camera.js`, `infer.js`, `collide.js`, `tools.js`, `sun.js`, the value box of `units.js`, `viewcube-camera.js`,
//! `terrain.js`, `dynamic.js` and `starter.js`, one for one, at exact parity with the frozen JS
//! (`forks/tools/tdiff.mjs` runs the same scripted sessions on both and compares every step at zero tolerance).
//!
//! - [`camera`]: the camera (projection, pick rays, orbit / pan / zoom / fit); [`sun`]: the sun for shadows.
//! - [`scene`]: what can be snapped to and picked, with box trees per definition; [`infer`]: inference snapping,
//!   picking, window selection, dimension and section-plane layouts; [`collide`]: solids that stop each other.
//! - [`tools`]: the Tools state machine fed pointer events, keys and the value box, editing the model through
//!   `Model::change` / `begin` / `preview` / `commit` / `abort` as the JS does, and describing an [`overlay::Overlay`].
//! - [`units`]: the value box's parsing ([`jsre`]: the JS regular expressions it runs).
//! - [`terrain`], [`dynamic`], [`starter`]: terrain from contours or a grid; components that stretch or repeat their
//!   parts; the starter component library.
//! - [`session`]: scripted sessions (`cadcraft-cli m3dtools`, the parity harness, the tests).
//!
//! No UI toolkit, no clock, no randomness: wasm-safe and deterministic. Spec gaps are written next to the code.

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
// counts and indices become JS numbers (f64) by design
#![allow(clippy::cast_precision_loss)]

pub mod camera;
pub mod catalog;
pub mod collide;
pub mod dynamic;
pub mod fuzzdrive;
pub mod i18n;
pub mod infer;
pub mod js;
pub mod jsre;
pub mod outj;
pub mod overlay;
pub mod scene;
pub mod session;
pub mod starter;
pub mod sun;
pub mod terrain;
pub mod tools;
pub mod units;
pub mod viewcube_camera;

pub use camera::Camera;
pub use tools::{Ev, Host, Tools};
