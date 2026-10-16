//! TEMPORARY stub: the port of BimCraft's dxf-model.js is in progress (replaced on merge).

use crate::value::Obj;

pub fn read_dxf_model(_text: &str) -> Obj {
    let mut m = crate::model::new_model();
    if let Some(r) = m.obj_mut("report") {
        r.set("error", "DXF reading is not ported yet");
    }
    m
}
