//! Opening a drawing: bytes → model (cc-dwg / DXF) → compiled drawing → GPU-ready geometry per space.
//! Pure (no egui): the CLI and tests use it too.

use std::sync::Arc;

use cc_drawing::Drawing;
use cc_render::{SpaceGeometry, build_geometry};

/// Largest file we open (the readers also cap what they allocate).
pub const MAX_FILE_BYTES: usize = 512 * 1024 * 1024;

#[derive(Debug)]
pub struct Loaded {
    pub name: String,
    pub drawing: Arc<Drawing>,
    pub geometry: Arc<Vec<SpaceGeometry>>,
    /// unique per load: GPU meshes are keyed by it
    pub id: u64,
    pub read_ms: f64,
    pub compile_ms: f64,
    pub geometry_ms: f64,
    pub notes: Vec<String>,
    pub bytes: usize,
}

fn next_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(1);
    N.fetch_add(1, Ordering::Relaxed)
}

fn ms(t: web_time::Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// Open a DWG or DXF from its bytes (the name decides DXF vs DWG; DWG is also sniffed by its "AC10" start).
pub fn open(name: &str, bytes: &[u8]) -> Result<Loaded, String> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(format!("{name}: the file is larger than {} MB", MAX_FILE_BYTES / (1024 * 1024)));
    }
    let is_dxf = name.to_ascii_lowercase().ends_with(".dxf") && !bytes.starts_with(b"AC10");
    let t = web_time::Instant::now();
    let model = if is_dxf {
        let text = cc_io_dwg::dxf_model::decode_dxf_bytes(bytes);
        cc_io_dwg::dxf_model::read_dxf_model(&text)
    } else {
        cc_io_dwg::read_dwg(bytes, &cc_io_dwg::ReadOpts::default())
    };
    let read_ms = ms(t);
    let report = model.obj("report");
    if let Some(e) = report.and_then(|r| r.str("error")) {
        return Err(format!("{name}: {e}"));
    }
    let notes: Vec<String> =
        report.and_then(|r| r.arr("notes")).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let version = model.str("version").unwrap_or("").to_string();
    let t = web_time::Instant::now();
    let drawing = cc_io_dwg::compile::compile_drawing(&model, &version, &[]);
    let compile_ms = ms(t);
    drop(model);
    let t = web_time::Instant::now();
    let geometry = build_all(&drawing);
    let geometry_ms = ms(t);
    Ok(Loaded {
        name: name.to_string(),
        drawing: Arc::new(drawing),
        geometry: Arc::new(geometry),
        id: next_id(),
        read_ms,
        compile_ms,
        geometry_ms,
        notes,
        bytes: bytes.len(),
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn build_all(d: &Drawing) -> Vec<SpaceGeometry> {
    use rayon::prelude::*;
    d.spaces.par_iter().map(build_geometry).collect()
}
#[cfg(target_arch = "wasm32")]
fn build_all(d: &Drawing) -> Vec<SpaceGeometry> {
    d.spaces.iter().map(build_geometry).collect()
}
