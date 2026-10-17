//! CadCraft L2: TakeoffCraft's engine — the Rust form of the DOM-free modules of TakeoffCraft 0.5
//! (`public_html/assets/apps/takeoffcraft/dev/`), ported one for one with the same order of floating-point operations,
//! V8's `Math` functions (fdlibm through `libm` and cc-m3d's `jsmath`) and `Number#toString` / `toFixed` text, and
//! checked against the frozen JS by `forks/tools/todiff.mjs` (`cadcraft-cli takeoff`) at zero tolerance.
//!
//! Canonical units: lengths in FEET, areas in square feet; a sheet's scale is `ftPerUnit` (real feet per PDF user-space
//! unit). Money: unit rates are integers in 1/10000 $, extensions integer cents, waste integer basis points.
//!
//! - [`units`], [`geom`] (arcs as bulges, exact lengths and areas), [`formula`] (parser + tree walker, no eval).
//! - [`model`]: the `Takeoff` (state as JSON in the JS key order, undo over snapshots), `readProject` (untrusted:
//!   every field checked, counts capped), quantities and the priced estimate; [`bid`]: the bid summary.
//! - [`library`], [`packs`], [`clipboard`]: price library CSV, trade packs, copy / paste.
//! - [`snap`], [`typein`], [`overlay`], [`matcher`] (auto count: NCC template matching), [`scaletext`], [`roomfill`],
//!   [`scene3d`]: the drawing aids.
//!
//! Where the JS relies on the engine (V8 error texts, lone UTF-16 surrogates, `Math.max(...hugeArray)`), the difference
//! is written next to the code as a "Spec gap".

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
// counts are JS numbers (f64) by design
#![allow(clippy::cast_precision_loss)]

pub mod bid;
pub mod clipboard;
pub mod collate;
pub mod formula;
pub mod geom;
pub mod i18n;
pub mod jv;
pub mod library;
pub mod matcher;
pub mod model;
pub mod overlay;
pub mod packs;
pub mod roomfill;
pub mod scaletext;
pub mod scene3d;
pub mod snap;
pub mod typein;
pub mod units;

/// V8's `Math` functions: fdlibm (FreeBSD msun) through `libm` and cc-m3d's port of sin / cos.
pub mod jsmath {
    #[inline]
    pub fn sin(x: f64) -> f64 {
        cc_m3d::jsmath::fd_sin(x)
    }
    #[inline]
    pub fn cos(x: f64) -> f64 {
        cc_m3d::jsmath::fd_cos(x)
    }
    #[inline]
    pub fn tan(x: f64) -> f64 {
        libm::tan(x)
    }
    #[inline]
    pub fn atan(x: f64) -> f64 {
        libm::atan(x)
    }
    #[inline]
    pub fn atan2(y: f64, x: f64) -> f64 {
        libm::atan2(y, x)
    }
    /// `Math.pow` / `**`: V8 special-cases y = 2 (x · x) and y = 0.5 (a square root, with ES's −0 and −∞), and the
    /// JS rule that ±1 to an infinite power is NaN; everything else is the C library's `pow` (glibc on Linux, which
    /// Node links), measured equal on 36 000 random cases. Spec gap: a wasm build uses Rust's own `pow`, a few last
    /// bits apart on some inputs.
    pub fn pow(x: f64, y: f64) -> f64 {
        if y.is_nan() {
            return f64::NAN;
        }
        if y.is_infinite() && x.abs() == 1.0 {
            return f64::NAN;
        }
        if y == 2.0 {
            return x * x;
        }
        if y == 0.5 {
            if x == f64::NEG_INFINITY {
                return f64::INFINITY;
            }
            if x == 0.0 {
                return 0.0;
            }
            return x.sqrt();
        }
        x.powf(y)
    }
}
