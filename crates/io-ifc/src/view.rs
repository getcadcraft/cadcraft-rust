//! The model for a 3D view: every stored mesh once and every product's pieces placing it (the instancing BimCraft's
//! viewer gets from IfcAPI.StreamAllMeshes, which hands out geometry ids and flat transformations). Z up, metres
//! (the file's length unit applied); no Y-up flip (the Rust viewer is Z up).

use crate::geo::bool_ops::SolidOps;
use crate::geo::ctx::GeoErr;
use crate::geo::geometry::Geometry;
use crate::geo::math::{M4, mul, scaling};
use crate::model::IfcFile;

/// One stored mesh: positions and normals (3 per vertex, the mesh's own frame), triangles.
#[derive(Clone, Debug, Default)]
pub struct ViewMesh {
    pub pos: Vec<f64>,
    pub nrm: Vec<f64>,
    pub idx: Vec<u32>,
}

/// One piece of a product: which mesh, where (world, Z up, metres), its colour (r, g, b, a in 0..1).
#[derive(Clone, Debug)]
pub struct ViewPiece {
    pub product: f64,
    pub ty: String,
    pub mesh: usize,
    pub matrix: M4,
    pub color: [f64; 4],
}

/// A whole file for viewing.
#[derive(Clone, Debug, Default)]
pub struct IfcView {
    pub meshes: Vec<ViewMesh>,
    pub pieces: Vec<ViewPiece>,
    pub products: usize,
    pub notes: Vec<String>,
}

/// Read and draw every product of a file (in id order) for the 3D view.
pub fn view(bytes: &[u8], ops: &dyn SolidOps) -> Result<IfcView, GeoErr> {
    let file = IfcFile::open(bytes).map_err(|_| GeoErr("Not an IFC file (no STEP data found)".into()))?;
    file.parse_all();
    let mut geo = Geometry::new(&file, ops)?;
    let ids = geo.products()?;
    let unit = geo.g.length_unit;
    let to_world = |m: &M4| mul(&scaling(unit, unit, unit), m);
    let mut out = IfcView::default();
    // stored mesh id → index in out.meshes
    let mut index: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for id in ids {
        let pcs = geo.pieces(id)?;
        if pcs.is_empty() {
            continue;
        }
        out.products = out.products.saturating_add(1);
        let ty = file.ty(id).filter(|t| !t.is_empty()).map(crate::schema::api_type_name).unwrap_or_default();
        for p in pcs {
            let Some(m) = geo.mesh(p.geom) else { continue };
            if m.idx.is_empty() {
                continue;
            }
            let mi = match index.get(&p.geom) {
                Some(i) => *i,
                None => {
                    out.meshes.push(ViewMesh { pos: m.pos.clone(), nrm: m.nrm.clone(), idx: m.idx.clone() });
                    let i = out.meshes.len().saturating_sub(1);
                    index.insert(p.geom, i);
                    i
                }
            };
            out.pieces.push(ViewPiece { product: id, ty: ty.clone(), mesh: mi, matrix: to_world(&p.matrix), color: p.color });
        }
    }
    out.notes = geo.notes().keys().cloned().collect();
    Ok(out)
}
