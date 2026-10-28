//! Elevation grids and LiDAR into CivilCraft, surfaces out as GeoTIFF (gis-terrain-ui.js).

use serde_json::Value;

use crate::civilcrs::PickFor;
use crate::civilview::CivilView;

/// The terrain import dialog.
#[derive(Clone, Debug, Default)]
pub struct TerrainDlg {}

/// The GeoTIFF export dialog.
#[derive(Clone, Debug, Default)]
pub struct GeoTiffDlg {}

impl CivilView {
    pub fn terrain_crs_answer(&mut self, _f: PickFor, _v: Option<Value>) {}
    pub fn ask_other(&mut self, what: &str, b: &str) {
        if what == "mapcrs" {
            self.pic_ask(b);
        }
    }
}
