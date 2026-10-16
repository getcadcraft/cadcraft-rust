//! The SURFACE family (port of lib/dwg/surface-read.js, surface-layout.js). STUB.

use crate::bits::Res;
use crate::objects::{Decoder, Record};
use crate::read::Build;
use crate::value::Obj;

pub fn decoder(_name: &str) -> Option<Decoder> {
    None
}
pub fn surface_fields(_rec: &Record, _f: &mut Obj, _b: &Build) -> Res<()> {
    Ok(())
}
