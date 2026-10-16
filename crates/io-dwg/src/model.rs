//! Model helpers (port of lib/dwg/model.js, read side): lineweights, the ACI palette, transparency, an empty model.

use crate::obj;
use crate::value::{Obj, V};

/// DWG lineweight index → 1/100 mm (DXF's set; model.js LINEWEIGHTS).
pub const LINEWEIGHTS: [i32; 24] = [0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200, 211];

pub fn lineweight_from_index(i: i64) -> i32 {
    match i {
        29 => -1,
        30 => -2,
        31 => -3,
        _ => usize::try_from(i).ok().and_then(|k| LINEWEIGHTS.get(k)).copied().unwrap_or(-1),
    }
}

/// DWG transparency word → { byLayer } | { byBlock } | { alpha }.
pub fn transparency_from_word(w: u32) -> Obj {
    match w >> 24 {
        0 => obj! { "byLayer" => true },
        1 => obj! { "byBlock" => true },
        _ => obj! { "alpha" => w & 0xFF },
    }
}

/// An empty model (newModel).
pub fn new_model() -> Obj {
    obj! {
        "version" => "AC1015", "maintenance" => 0, "codepage" => "ANSI_1252", "header" => Obj::new(), "classes" => Vec::<V>::new(),
        "tables" => obj! {
            "layers" => Vec::<V>::new(), "linetypes" => Vec::<V>::new(), "styles" => Vec::<V>::new(), "dimstyles" => Vec::<V>::new(),
            "views" => Vec::<V>::new(), "ucs" => Vec::<V>::new(), "vports" => Vec::<V>::new(), "appids" => Vec::<V>::new(),
            "blockRecords" => Vec::<V>::new(),
        },
        "blocks" => Vec::<V>::new(), "layouts" => Vec::<V>::new(), "objects" => Vec::<V>::new(),
        "report" => obj! { "counts" => Obj::new(), "notes" => Vec::<V>::new(), "unknown" => Obj::new() },
    }
}
