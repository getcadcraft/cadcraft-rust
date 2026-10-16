//! MULTILEADER and MLEADERSTYLE into the model (port of lib/dwg/mleader-read.js). STUB.

use crate::bits::Res;
use crate::objects::Record;
use crate::read::Build;
use crate::value::Obj;

pub fn read_mleader(_rec: &Record, _b: &Build) -> Res<Option<Obj>> {
    Ok(None)
}
pub fn read_mleader_style(_rec: &Record, _b: &Build) -> Res<Option<Obj>> {
    Ok(None)
}
