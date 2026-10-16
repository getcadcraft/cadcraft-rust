//! REGION / 3DSOLID / BODY and the R2013+ data storage (port of lib/dwg/solid-read.js, acis.js, acds.js). STUB.

use crate::objects::{Decoder, Record};
use crate::read::{Build, Records};
use crate::value::Obj;

pub fn decoder(_name: &str) -> Option<Decoder> {
    None
}
pub fn attach_data_storage(_records: &mut Records, _sec: Option<&[u8]>, _notes: &mut Vec<String>) {}
pub fn solid_fields(_rec: &Record, _f: &mut Obj, _b: &Build) {}
