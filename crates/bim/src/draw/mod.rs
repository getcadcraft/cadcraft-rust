//! BimCraft's building tools (CadCraft 0.5 part 3, track BM; `model-*.js`, `new-model.js`, `ifc-edit.js`, the parts of
//! `res-roof.js` / `res-skeleton.js` the Roof tool uses), one for one with the frozen JS, writing through cc-ifc's
//! IfcAPI ([`crate::wapi`]). No DOM, no clock, no randomness of their own: GlobalIds come from the model's seeded
//! [`crate::wapi::Rand`], dates from the caller.

pub mod editor;
pub mod job;
pub mod levels;
pub mod m;
pub mod newmodel;
pub mod openings;
pub mod ops;
pub mod pdf;
pub mod plan;
pub mod roof;
pub mod rooms;
pub mod skeleton;
pub mod slabs;
pub mod tools;
pub mod tr;
pub mod units;
pub mod walls;
pub mod writer;
