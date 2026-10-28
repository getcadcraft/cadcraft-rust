//! CadCraft L5: the session's command engine — the CadCraft app's (0.5) commands.js command line, M4.
//!
//! [`prompt`] is the question machinery the commands are written in: `async` command bodies awaiting answers,
//! driven by [`prompt::Runner`] without a runtime (desktop and wasm alike). [`engine::Engine`] is the command line
//! (typed text, clicks, selection windows, Esc; repeat-last on Enter; one undo step per command) over a
//! [`engine::Host`]; [`cmd`] holds the drafting commands ported so far: M4's LINE PLINE CIRCLE (2P 3P TTR TTT) ARC
//! RECTANG POLYGON ELLIPSE POINT TEXT ERASE MOVE COPY ROTATE SCALE MIRROR OFFSET TRIM EXTEND UNDO REDO ZOOM, and since
//! M4.10 HATCH (pick inside), the DIM family (linear, aligned, radius, diameter, angular, continue, baseline, the dim
//! variables), FILLET CHAMFER EXPLODE STRETCH ARRAY BREAK LENGTHEN JOIN MATCHPROP XLINE RAY MTEXT REVCLOUD MLEADER
//! TEXTEDIT, BLOCK ATTDEF INSERT BEDIT BCLOSE PURGE, DIST AREA MEASUREGEOM ID LIST SELECT DUPLICATE, the layer tools
//! and RENAME LINETYPE LTSCALE LWEIGHT, PROPERTIES (M4.11, [`props`]: propdefs.js / propexpr.js) and PLOT (M4.12).
//! Parity with the frozen JS is checked by `forks/tools/kdiff.mjs` (`cadcraft-cli cmds`).
//!
//! The parametric commands (pcommands.js: GEOMCONSTRAINT / GC*, DIMCONSTRAINT / DC*, DELCONSTRAINT, AUTOCONSTRAIN,
//! PARAMETERS, -PARAMETERS, CONSTRAINTBAR, DCDISPLAY; M4.23) work through cc-gcons; MIRROR, TRIM and the clipboard
//! carry constraints (ckeep.js). The 3D tab ([`ws3d`], ws3d-core.js: ModelCraft's tools on the drawing's 3D model) runs
//! PRESSPULL, 3DORBIT, VIEW, VSCURRENT, the booleans and the 3D versions of MOVE / COPY / ROTATE / ERASE / ZOOM (M4.25).
//! Not ported here: SWBOUT (swsketch.js), lasso selection, dynamic-block properties (the Custom section).

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

pub mod clip;
pub mod cmd;
pub mod compare;
pub mod engine;
pub mod input;
pub mod lang;
pub mod lasso;
pub mod lisp;
pub mod prompt;
pub mod propextra;
pub mod props;
pub mod sheetset;
pub mod sheetset_io;
pub mod shortcuts;
pub mod tancircle;
pub mod ucs;
pub mod ws3d;

pub use engine::{Ctx, Engine, Host, IdSet, Opts, Pick, PickedDrawing, PickedFile, Reply, Sel, Tan};
pub use lang::Lang;
