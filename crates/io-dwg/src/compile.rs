//! TEMPORARY stub: the port of BimCraft's dwg-compile.js is in progress (replaced on merge).

use crate::value::Obj;

pub fn compile_drawing(_model: &Obj, version_tag: &str, _xrefs: &[(String, Obj)]) -> cc_drawing::Drawing {
    cc_drawing::Drawing {
        version: version_tag.to_string(),
        release: String::new(),
        units: String::new(),
        xrefs: vec![],
        images: vec![],
        layers: vec![],
        spaces: vec![],
        stats: cc_drawing::Stats::default(),
    }
}
