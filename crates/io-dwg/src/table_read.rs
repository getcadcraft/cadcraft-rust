//! ACAD_TABLE and its objects (port of lib/dwg/table-read.js). STUB: being ported.

use crate::bits::Res;
use crate::objects::{Decoder, Record};
use crate::read::Build;
use crate::value::Obj;

pub fn decoder(_name: &str) -> Option<Decoder> {
    None
}
pub fn table_entity_fields(_rec: &Record, _f: &mut Obj, _b: &Build) -> Res<()> {
    Ok(())
}
pub fn table_object_fields(_rec: &Record, _f: &mut Obj, _b: &Build) -> Res<()> {
    Ok(())
}
