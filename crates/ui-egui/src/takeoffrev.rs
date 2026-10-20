//! TakeoffCraft's revision overlay (M9.22 fills this in).

use cc_takeoff::geom::P;
use egui::Pos2;

use crate::takeofftools::TakeoffMode;

/// The revision overlay.
#[derive(Debug, Clone, Default)]
pub struct Cmp {
    pub align: Option<Vec<P>>,
}

impl TakeoffMode {
    pub(crate) fn align_pick(&mut self, _p: P) {}
    pub(crate) fn draw_compare(&self, _painter: &egui::Painter, _sheet: &str, _at: &dyn Fn(f64, f64) -> Pos2) {}
}
