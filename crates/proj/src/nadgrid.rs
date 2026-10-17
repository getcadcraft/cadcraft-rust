//! proj4js `nadgrid.js`: NTv2 grid shift files and PROJ GeoTIFF grids, registered by name (`+nadgrids=@name`).
//! Files are untrusted: every offset and count is checked against the bytes before it is read.

use crate::val::Val;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

/// A subgrid: lower-left corner and spacing in radians (longitudes positive west), node counts, shifts per node.
#[derive(Clone, Debug, PartialEq)]
pub struct Subgrid {
    pub ll: [f64; 2],
    pub del: [f64; 2],
    pub lim: [f64; 2],
    pub count: Option<f64>,
    pub cvs: Vec<[f64; 2]>,
}

/// A loaded grid.
#[derive(Clone, Debug, PartialEq)]
pub struct NadGrid {
    pub n_subgrids: f64,
    pub subgrids: Vec<Subgrid>,
}

/// `{ name, mandatory, grid, isNull }`.
#[derive(Clone, Debug)]
pub struct GridRef {
    pub name: String,
    pub mandatory: bool,
    pub grid: Option<Arc<NadGrid>>,
    pub is_null: bool,
}

fn registry() -> &'static Mutex<HashMap<String, Arc<NadGrid>>> {
    static R: OnceLock<Mutex<HashMap<String, Arc<NadGrid>>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

/// `loadedNadgrids[key] = grid`.
pub fn register(key: &str, g: NadGrid) -> Arc<NadGrid> {
    let a = Arc::new(g);
    registry().lock().unwrap_or_else(PoisonError::into_inner).insert(key.to_string(), a.clone());
    a
}
pub fn loaded(key: &str) -> Option<Arc<NadGrid>> {
    registry().lock().unwrap_or_else(PoisonError::into_inner).get(key).cloned()
}

/// `getNadgrids(nadgrids)`: None when undefined.
pub fn get_nadgrids(v: &Val) -> Result<Option<Vec<Option<GridRef>>>, String> {
    match v {
        Val::Undef => Ok(None),
        Val::Str(s) => Ok(Some(s.split(',').map(parse_nadgrid_string).collect())),
        other => Err(crate::defs::type_error_call(other, "split")),
    }
}

fn parse_nadgrid_string(value: &str) -> Option<GridRef> {
    if value.is_empty() {
        return None;
    }
    let optional = value.starts_with('@');
    let v = if optional { value.get(1..).unwrap_or("") } else { value };
    if v == "null" {
        return Some(GridRef { name: "null".into(), mandatory: !optional, grid: None, is_null: true });
    }
    Some(GridRef { name: v.to_string(), mandatory: !optional, grid: loaded(v), is_null: false })
}

fn sec_to_rad(s: f64) -> f64 {
    (s / 3600.0) * std::f64::consts::PI / 180.0
}
fn deg_to_rad(d: f64) -> f64 {
    d * std::f64::consts::PI / 180.0
}

struct View<'a> {
    b: &'a [u8],
    le: bool,
}
impl View<'_> {
    fn bytes<const N: usize>(&self, off: usize) -> Result<[u8; N], String> {
        let s = off
            .checked_add(N)
            .and_then(|e| self.b.get(off..e))
            .ok_or_else(|| "Offset is outside the bounds of the DataView".to_string())?;
        let mut a = [0u8; N];
        a.copy_from_slice(s);
        Ok(a)
    }
    fn i32(&self, off: usize, le: bool) -> Result<i32, String> {
        let a = self.bytes::<4>(off)?;
        Ok(if le { i32::from_le_bytes(a) } else { i32::from_be_bytes(a) })
    }
    fn f64(&self, off: usize) -> Result<f64, String> {
        let a = self.bytes::<8>(off)?;
        Ok(if self.le { f64::from_le_bytes(a) } else { f64::from_be_bytes(a) })
    }
    fn f32(&self, off: usize) -> Result<f64, String> {
        let a = self.bytes::<4>(off)?;
        Ok(f64::from(if self.le { f32::from_le_bytes(a) } else { f32::from_be_bytes(a) }))
    }
    fn string(&self, start: usize, end: usize) -> String {
        // String.fromCharCode over the bytes (Latin-1), slice clamped as ArrayBuffer.slice clamps
        let e = end.min(self.b.len());
        let s = start.min(e);
        self.b.get(s..e).unwrap_or(&[]).iter().map(|&c| char::from(c)).collect()
    }
}

/// The most nodes a grid file may declare (all subgrids): 50 million (the largest PROJ grids hold a few million).
const MAX_NODES: f64 = 5.0e7;

/// `nadgrid(key, ArrayBuffer)` for an NTv2 file (with its error fields).
pub fn read_ntv2(key: &str, data: &[u8]) -> Result<Arc<NadGrid>, String> {
    let probe = View { b: data, le: false };
    let le = probe.i32(8, false)? != 11;
    let v = View { b: data, le };
    let n_subgrids = v.i32(40, le)?;
    v.f64(168)?; // the header is 176 bytes (the JS reads every field)
    let _shift = v.string(56, 64);
    let mut subgrids = Vec::new();
    let mut off: usize = 176;
    let mut total = 0.0f64;
    for _ in 0..n_subgrids.max(0) {
        let lower_lat = v.f64(off.saturating_add(72))?;
        let upper_lat = v.f64(off.saturating_add(88))?;
        let lower_lon = v.f64(off.saturating_add(104))?;
        let upper_lon = v.f64(off.saturating_add(120))?;
        let lat_int = v.f64(off.saturating_add(136))?;
        let lon_int = v.f64(off.saturating_add(152))?;
        let count = v.i32(off.saturating_add(168), le)?;
        total += f64::from(count.max(0));
        if total > MAX_NODES {
            return Err("Offset is outside the bounds of the DataView".into());
        }
        let nodes_off = off.saturating_add(176);
        let n = usize::try_from(count.max(0)).unwrap_or(0);
        if nodes_off.saturating_add(n.saturating_mul(16)) > data.len() && n > 0 {
            return Err("Offset is outside the bounds of the DataView".into());
        }
        let mut cvs = Vec::with_capacity(n);
        for i in 0..n {
            let at = nodes_off.saturating_add(i.saturating_mul(16));
            let lat_shift = v.f32(at)?;
            let lon_shift = v.f32(at.saturating_add(4))?;
            cvs.push([sec_to_rad(lon_shift), sec_to_rad(lat_shift)]);
        }
        let lng_cols = crate::math::round(1.0 + (upper_lon - lower_lon) / lon_int);
        let lat_cols = crate::math::round(1.0 + (upper_lat - lower_lat) / lat_int);
        subgrids.push(Subgrid {
            ll: [sec_to_rad(lower_lon), sec_to_rad(lower_lat)],
            del: [sec_to_rad(lon_int), sec_to_rad(lat_int)],
            lim: [lng_cols, lat_cols],
            count: Some(f64::from(count)),
            cvs,
        });
        off = off.saturating_add(176).saturating_add(n.saturating_mul(16));
    }
    let g = NadGrid { n_subgrids: f64::from(n_subgrids), subgrids };
    Ok(register(key, g))
}

/// `readGeotiffGrid(key, tiff)`: subgrids from the images last to first; band 0 latitude, band 1 longitude offsets
/// (arc seconds).
pub fn read_geotiff_grid(key: &str, tiff: &crate::tiff::Tiff) -> Result<Arc<NadGrid>, String> {
    let count = tiff.image_count();
    let mut subgrids = Vec::new();
    let mut nodes_total = 0.0f64;
    for idx in (0..count).rev() {
        let image = tiff.image(idx)?;
        let rasters = image.read_rasters(&crate::tiff::ReadOptions::default())?;
        let w = image.width();
        let h = image.height();
        nodes_total += w * h;
        if nodes_total > MAX_NODES {
            return Err("grid too large".into());
        }
        let bbox = image.bounding_box(false)?;
        let bb: Vec<f64> = bbox.iter().map(|&d| deg_to_rad(d)).collect();
        let mps = image.model_pixel_scale().ok_or_else(|| "Cannot read properties of undefined (reading '0')".to_string())?;
        let del = [deg_to_rad(mps.first().copied().unwrap_or(f64::NAN)), deg_to_rad(mps.get(1).copied().unwrap_or(f64::NAN))];
        let max_x = bb.first().copied().unwrap_or(f64::NAN) + (w - 1.0) * del[0];
        let min_y = bb.get(3).copied().unwrap_or(f64::NAN) - (h - 1.0) * del[1];
        let lat_band = rasters.bands.first().ok_or("no band 0")?;
        let lon_band = rasters.bands.get(1).ok_or("no band 1")?;
        let wi = crate::tiff::to_usize(w);
        let hi = crate::tiff::to_usize(h);
        let mut nodes = Vec::with_capacity(wi.saturating_mul(hi));
        for i in (0..hi).rev() {
            for j in (0..wi).rev() {
                let index = i.saturating_mul(wi).saturating_add(j);
                let lon = lon_band.get(index).copied().unwrap_or(f64::NAN);
                let lat = lat_band.get(index).copied().unwrap_or(f64::NAN);
                nodes.push([-sec_to_rad(lon), sec_to_rad(lat)]);
            }
        }
        subgrids.push(Subgrid { del, lim: [w, h], ll: [-max_x, min_y], count: None, cvs: nodes });
    }
    let g = NadGrid { n_subgrids: count as f64, subgrids };
    Ok(register(key, g))
}
