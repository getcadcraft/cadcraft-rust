//! The parity kit's `ifc-geom` engine (tools/fork-parity/lib/engines.mjs): every product IfcAPI.StreamAllMeshes gives,
//! as {type, geometries, vertices, triangles, bbox [minx, miny, minz, maxx, maxy, maxz], area} in metres, Y up, after
//! the flat transformation; vertices go through a Float32Array as GetVertexArray hands them out, triangles are counted
//! per piece and area is the sum of triangle areas.

use crate::geo::bool_ops::SolidOps;
use crate::geo::ctx::GeoErr;
use crate::geo::geometry::Geometry;
use crate::geo::math::{I4, M4, mul, scaling};
use crate::jsmath::hypot3;
use crate::model::IfcFile;
use crate::util::{f32_round, push_json_num, push_json_str, usize_f64};

/// Z up → Y up (api.js YUP).
const YUP: M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0];

/// One product's summary.
#[derive(Clone, Debug, PartialEq)]
pub struct Product {
    pub id: f64,
    pub ty: String,
    pub geometries: usize,
    pub vertices: usize,
    pub triangles: usize,
    pub area: f64,
    pub bbox: Option<[f64; 6]>,
}

/// Every drawn product of a file (in id order); Err when the JS IfcAPI throws (a file that is not STEP, a line it
/// must read that does not parse).
pub fn products(bytes: &[u8], ops: &dyn SolidOps) -> Result<Vec<Product>, GeoErr> {
    let mut file = IfcFile::open(bytes).map_err(|_| GeoErr("No IFC model is open with id -1".into()))?;
    // every line parsed up front, in parallel (a line read later finds its result; one that does not parse fails the
    // same reads it would fail when parsed on demand)
    file.parse_all();
    let out = summarise(&file, ops);
    // the parsed lines are freed off the caller's time
    let parsed = file.take_parsed();
    #[cfg(not(target_arch = "wasm32"))]
    if std::thread::Builder::new().spawn(move || drop(parsed)).is_err() {
        // could not start a thread: dropped here instead
    }
    #[cfg(target_arch = "wasm32")]
    drop(parsed);
    out
}

/// Products per worker below which the work stays on one thread.
#[cfg(not(target_arch = "wasm32"))]
const MIN_PARALLEL: usize = 64;

fn summarise(file: &IfcFile<'_>, ops: &dyn SolidOps) -> Result<Vec<Product>, GeoErr> {
    let ids = Geometry::new(file, ops)?.products()?;
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(pool) = pool()
        && ids.len() >= MIN_PARALLEL.saturating_mul(2)
    {
        // each worker its own Geometry (item meshes and placements are pure functions of their ids, so a cache built
        // by another worker would hold the same values), over a contiguous run of products; output in id order
        use rayon::prelude::*;
        let per = ids.len().div_ceil(pool.current_num_threads().max(1)).max(MIN_PARALLEL);
        let parts: Vec<Result<(Vec<Product>, bool), GeoErr>> = pool.install(|| ids.par_chunks(per).map(|c| run(file, ops, c)).collect());
        let mut out = Vec::with_capacity(ids.len());
        let mut order_sensitive = false;
        for p in parts {
            let (v, sens) = p?;
            order_sensitive |= sens;
            out.extend(v);
        }
        // a placement loop or a call-stack limit gives results that depend on which product met it first: then the
        // products are drawn in one pass, in order, as the JS does
        if !order_sensitive {
            return Ok(out);
        }
    }
    run(file, ops, &ids).map(|(v, _)| v)
}

/// The workers' pool: deep stacks (boolean trees nest as deep as a file makes them).
#[cfg(not(target_arch = "wasm32"))]
fn pool() -> Option<&'static rayon::ThreadPool> {
    static POOL: std::sync::OnceLock<Option<rayon::ThreadPool>> = std::sync::OnceLock::new();
    POOL.get_or_init(|| rayon::ThreadPoolBuilder::new().stack_size(64 << 20).thread_name(|i| format!("cc-ifc-geom-{i}")).build().ok())
        .as_ref()
        .filter(|p| p.current_num_threads() > 1)
}

/// Draw and summarise a run of products with one Geometry; → (products, whether the results could depend on order).
fn run(file: &IfcFile<'_>, ops: &dyn SolidOps, ids: &[crate::model::Id]) -> Result<(Vec<Product>, bool), GeoErr> {
    let mut geo = Geometry::new(file, ops)?;
    let unit = geo.g.length_unit;
    let to_world = |m: &M4| mul(&I4, &mul(&YUP, &mul(&scaling(unit, unit, unit), m)));
    let mut out = Vec::with_capacity(ids.len());
    for &id in ids {
        let pcs = geo.pieces(id)?;
        if pcs.is_empty() {
            continue;
        }
        let ty = file.ty(id).filter(|t| !t.is_empty()).map(crate::schema::api_type_name).unwrap_or_default();
        let mut p = Product { id, ty, geometries: 0, vertices: 0, triangles: 0, area: 0.0, bbox: None };
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for piece in &pcs {
            let Some(mesh) = geo.mesh(piece.geom) else { continue };
            let m = to_world(&piece.matrix);
            let pts: Vec<[f64; 3]> = mesh
                .pos
                .as_chunks::<3>()
                .0
                .iter()
                .map(|v| {
                    let (x, y, z) = (f32_round(v[0]), f32_round(v[1]), f32_round(v[2]));
                    [
                        m[0] * x + m[4] * y + m[8] * z + m[12],
                        m[1] * x + m[5] * y + m[9] * z + m[13],
                        m[2] * x + m[6] * y + m[10] * z + m[14],
                    ]
                })
                .collect();
            for w in &pts {
                for (k, &c) in w.iter().enumerate() {
                    if let (Some(l), Some(h)) = (lo.get_mut(k), hi.get_mut(k)) {
                        if c < *l {
                            *l = c;
                        }
                        if c > *h {
                            *h = c;
                        }
                    }
                }
            }
            for t in mesh.idx.as_chunks::<3>().0.iter() {
                let g = |k: u32| usize::try_from(k).ok().and_then(|k| pts.get(k));
                let (Some(a), Some(b), Some(c)) = (g(t[0]), g(t[1]), g(t[2])) else { continue };
                let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let w = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                p.area += 0.5 * hypot3(u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]);
            }
            p.geometries = p.geometries.saturating_add(1);
            p.vertices = p.vertices.saturating_add(pts.len());
            p.triangles = p.triangles.saturating_add(mesh.idx.len() / 3);
        }
        p.bbox = if p.vertices > 0 { Some([lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]]) } else { None };
        out.push(p);
    }
    let sensitive = geo.notes().keys().any(|n| n == "A placement chain loops" || n.contains("Maximum call stack size exceeded"));
    Ok((out, sensitive))
}

/// The engine output as JSON: {"products": {"<id>": {…}}}.
pub fn to_json(products: &[Product]) -> String {
    let mut o = String::with_capacity(products.len().saturating_mul(160).saturating_add(16));
    o.push_str("{\"products\":{");
    for (k, p) in products.iter().enumerate() {
        if k > 0 {
            o.push(',');
        }
        o.push('"');
        push_json_num(&mut o, p.id);
        o.push_str("\":{\"area\":");
        push_json_num(&mut o, p.area);
        o.push_str(",\"bbox\":");
        match &p.bbox {
            Some(b) => {
                o.push('[');
                for (i, v) in b.iter().enumerate() {
                    if i > 0 {
                        o.push(',');
                    }
                    push_json_num(&mut o, *v);
                }
                o.push(']');
            }
            None => o.push_str("null"),
        }
        o.push_str(",\"geometries\":");
        push_json_num(&mut o, usize_f64(p.geometries));
        o.push_str(",\"triangles\":");
        push_json_num(&mut o, usize_f64(p.triangles));
        o.push_str(",\"type\":");
        push_json_str(&mut o, &p.ty);
        o.push_str(",\"vertices\":");
        push_json_num(&mut o, usize_f64(p.vertices));
        o.push('}');
    }
    o.push_str("}}");
    o
}
