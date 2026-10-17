//! ifc-model.js `IfcModel.load()` — what BimCraft's viewer and every module after it read from an open file: the
//! spatial tree (`api.properties.getSpatialStructure`), the storeys in elevation order, element → storey, the class
//! of every element with geometry, and the geometry merged into batches (one per storey × colour, spaces apart) with
//! each element's run of triangles and vertices, as Float32 / Uint32 arrays exactly as the JS builds them.
//!
//! Geometry comes from `cc-io-ifc`'s [`Geometry`] (the port of cc-ifc's geometry.js, booleans through
//! [`SolidOps`]); the stream order, the coordination matrix (`COORDINATE_TO_ORIGIN`), the Y-up flip and the
//! per-vertex arithmetic follow api.js `stream` / `flatMesh` / `ensureC` and ifc-model.js `appendGeometry`.

use std::collections::{HashMap, HashSet};

use cc_io_ifc::geo::bool_ops::SolidOps;
use cc_io_ifc::geo::geometry::Geometry;
use cc_io_ifc::geo::math::{I4, M4, mul, scaling, translation};
use cc_io_ifc::model::{Id, IfcFile, id_key};
use cc_io_ifc::value::Value;

use crate::api::{JsErr, Prim, R, class_name, get_line, val_model};
use crate::js::{OMap, PV, f32_of, hypot3, num_str, to_fixed};

/// Keys per model (ifc-model.js KEY_SPAN = 2^26).
pub const KEY_SPAN: f64 = 67_108_864.0;
/// The storey key of elements outside any storey.
const OTHER: f64 = -1.0;
/// web-ifc geometry is Y up: IFC (x, y, z) → (x, z, −y).
const YUP: M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0];
/// Depth of the spatial tree walk (api.js getSpatialStructure).
const MAX_TREE_DEPTH: u32 = 64;

/// A node of the spatial structure: `{ expressID, type, children }` (the root's type upper case).
#[derive(Clone, Debug)]
pub struct TNode {
    pub id: Id,
    pub ty: String,
    pub children: Vec<TNode>,
}

#[derive(Clone, Debug)]
pub struct Storey {
    pub id: Id,
    pub name: PV,
    pub elevation: Option<f64>,
}

/// One element's run in a batch.
#[derive(Clone, Debug, Default)]
pub struct Runs {
    pub ids: Vec<u32>,
    pub t0: Vec<u32>,
    pub tn: Vec<u32>,
    pub v0: Vec<u32>,
    pub vn: Vec<u32>,
}

/// One merged mesh (`finishBatch`).
#[derive(Clone, Debug, Default)]
pub struct Batch {
    pub storey: f64,
    pub space: bool,
    pub color: [f64; 4],
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub indices: Vec<u32>,
    pub tri_ids: Vec<u32>,
    pub elems: Runs,
}

/// The load result.
#[derive(Clone, Debug, Default)]
pub struct Model {
    pub key_base: f64,
    pub schema: String,
    pub storeys: Vec<Storey>,
    /// expressID → storey expressID (insertion order of the walk)
    pub storey_of: OMap<u64, f64>,
    /// expressID → class, every element with geometry (stream order, then the spaces)
    pub class_of: OMap<u64, String>,
    /// the ids of `class_of` as numbers, same order
    pub class_ids: Vec<Id>,
    pub tree: Option<TNode>,
    pub batches: Vec<Batch>,
    pub elements: usize,
    pub spaces: usize,
    pub triangles: f64,
    pub types: Vec<(String, usize)>,
    /// GetCoordinationMatrix
    pub coord: M4,
}

impl Model {
    pub fn class(&self, id: Id) -> Option<&str> {
        self.class_of.get(&id_key(id)).map(String::as_str)
    }
    pub fn storey(&self, id: Id) -> Option<f64> {
        self.storey_of.get(&id_key(id)).copied()
    }
}

/// JS `ToUint32` for a key stored in a Uint32Array.
pub fn u32_of(x: f64) -> u32 {
    if !x.is_finite() {
        return 0;
    }
    let t = x.trunc();
    let m = t.rem_euclid(4_294_967_296.0);
    // 0 ≤ m < 2^32: exact
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let v = m as u32;
    v
}

fn usize_f64(n: usize) -> f64 {
    u32::try_from(n).map_or(n as f64, f64::from)
}

type KidsFn<'k> = dyn Fn(Id, &str, &HashMap<u64, Vec<Id>>) -> R<Vec<Id>> + 'k;

/// `api.properties.getSpatialStructure(mid, false)`; Err = it threw (the caller keeps `tree = null`).
pub fn spatial_structure(f: &IfcFile<'_>) -> R<Option<TNode>> {
    let Some(&proj) = f.of_type("IFCPROJECT", false).first() else { return Ok(None) };
    let agg = f.inverse("IFCRELAGGREGATES", "RelatingObject").map_err(|e| JsErr(e.0))?;
    let cont = f.inverse("IFCRELCONTAINEDINSPATIALSTRUCTURE", "RelatingStructure").map_err(|e| JsErr(e.0))?;
    let kids = |id: Id, attr: &str, map: &HashMap<u64, Vec<Id>>| -> R<Vec<Id>> {
        let mut out = Vec::new();
        for &r in map.get(&id_key(id)).map(Vec::as_slice).unwrap_or(&[]) {
            match f.attr(r, attr).map_err(|e| JsErr(e.0))? {
                None | Some(Value::Null) => {}
                Some(Value::List(l)) => {
                    for x in l.iter() {
                        if let Value::Ref(c) = x
                            && *c != 0.0
                            && !c.is_nan()
                        {
                            out.push(*c);
                        }
                    }
                }
                // a string iterates its characters (none has `.ref`)
                Some(Value::Str(_)) => {}
                Some(_) => return Err(crate::api::type_err("object is not iterable")),
            }
        }
        Ok(out)
    };
    let mut seen: HashSet<u64> = HashSet::new();
    // explicit recursion with a depth bound of 64 (as the JS)
    fn node(
        f: &IfcFile<'_>,
        id: Id,
        depth: u32,
        seen: &mut HashSet<u64>,
        kids: &KidsFn<'_>,
        agg: &HashMap<u64, Vec<Id>>,
        cont: &HashMap<u64, Vec<Id>>,
    ) -> R<TNode> {
        seen.insert(id_key(id));
        let up = f.ty(id).unwrap_or("");
        let ty = if depth == 0 { up.to_string() } else { f.schema.pretty(up).filter(|p| !p.is_empty()).unwrap_or(up).to_string() };
        let mut n = TNode { id, ty, children: Vec::new() };
        if depth < MAX_TREE_DEPTH {
            let mut list = kids(id, "RelatedObjects", agg)?;
            list.extend(kids(id, "RelatedElements", cont)?);
            for c in list {
                if !seen.contains(&id_key(c)) && f.has(c) {
                    n.children.push(node(f, c, depth.saturating_add(1), seen, kids, agg, cont)?);
                }
            }
        }
        Ok(n)
    }
    node(f, proj, 0, &mut seen, &kids, &agg, &cont).map(Some)
}

/// `num(v)` of ifc-model.js: `Number(val(v))` when finite (null and undefined attributes are 0, as Number(null)).
fn num_of(v: Option<&crate::api::AV>) -> Option<f64> {
    use crate::api::AV;
    let n = match v {
        None | Some(AV::Null | AV::Derived) => 0.0,
        Some(AV::Handle(n)) => *n,
        Some(AV::List(l)) => {
            if l.is_empty() {
                0.0
            } else {
                f64::NAN
            }
        }
        Some(AV::Val(p)) => match p {
            Prim::Num(n) => *n,
            Prim::Str(s) => crate::js::str_num(s),
            Prim::Bool(b) => f64::from(u8::from(*b)),
            Prim::Arr(a) => match a.as_slice() {
                [] => 0.0,
                [x] => crate::js::str_num(&x.to_js_string()),
                _ => f64::NAN,
            },
            Prim::Null => 0.0,
            Prim::Undef | Prim::Obj => f64::NAN,
        },
    };
    n.is_finite().then_some(n)
}

fn prim_pv(p: &Prim) -> PV {
    match p {
        Prim::Num(n) => PV::Num(*n),
        other => PV::s(&other.to_js_string()),
    }
}

/// Storeys and storeyOf from the tree (`spatial()`).
fn spatial(f: &IfcFile<'_>, tree: Option<&TNode>) -> R<(Vec<Storey>, OMap<u64, f64>)> {
    let mut storeys = Vec::new();
    let mut storey_of = OMap::new();
    // iterative walk in the JS's pre-order
    let mut stack: Vec<(&TNode, Option<f64>)> = Vec::new();
    if let Some(t) = tree {
        stack.push((t, None));
    }
    while let Some((node, storey)) = stack.pop() {
        let mut here = storey;
        let is_storey = node.ty.to_uppercase() == "IFCBUILDINGSTOREY";
        if is_storey {
            here = Some(node.id);
            let s = get_line(f, node.id)?;
            let name = match &s {
                Some(l) => val_model(l.get("Name").as_ref())
                    .filter(truthy_prim)
                    .or_else(|| val_model(l.get("LongName").as_ref()).filter(truthy_prim))
                    .map(|p| prim_pv(&p)),
                // `s.Name` on null throws
                None => return Err(crate::api::type_err("Cannot read properties of null (reading 'Name')")),
            }
            .unwrap_or_else(|| PV::s(&format!("Storey {}", num_str(node.id))));
            let elevation = s.and_then(|l| num_of(l.get("Elevation").as_ref()));
            storeys.push(Storey { id: node.id, name, elevation });
        }
        if let Some(h) = here
            && !is_storey
        {
            storey_of.set(id_key(node.id), h);
        }
        for ch in node.children.iter().rev() {
            stack.push((ch, here));
        }
    }
    storeys.sort_by(|a, b| crate::js::num_cmp(a.elevation.unwrap_or(0.0), b.elevation.unwrap_or(0.0)));
    Ok((storeys, storey_of))
}

fn truthy_prim(p: &Prim) -> bool {
    match p {
        Prim::Undef | Prim::Null => false,
        Prim::Bool(b) => *b,
        Prim::Num(n) => *n != 0.0 && !n.is_nan(),
        Prim::Str(s) => !s.is_empty(),
        Prim::Arr(_) | Prim::Obj => true,
    }
}

/// `geo.products({ types: ['IFCSPACE'], exclude: [] })`.
fn space_products(f: &IfcFile<'_>) -> R<Vec<Id>> {
    let s = f.schema;
    let mut out = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let roots = s.subtypes("IFCSPACE");
    for t in roots.iter() {
        if !seen.insert(t) {
            continue;
        }
        for id in f.of_type(t, false) {
            let rep = f.attr(id, "Representation").map_err(|e| JsErr(e.0))?;
            if rep.is_some_and(Value::truthy) && f.attr(id, "ObjectPlacement").map_err(|e| JsErr(e.0))?.is_some() {
                out.push(id);
            }
        }
    }
    out.sort_by(|a, b| crate::js::num_cmp(*a, *b));
    Ok(out)
}

struct Acc {
    b: Batch,
    vcount: f64,
    tri_count: usize,
}

/// `ifc.load(bytes, …, { keyBase })` on an open file.
pub fn load(f: &IfcFile<'_>, ops: &dyn SolidOps, key_base: f64) -> R<Model> {
    let geo_err = |e: cc_io_ifc::geo::ctx::GeoErr| JsErr(e.0);
    let tree = spatial_structure(f).unwrap_or(None);
    let (storeys, storey_of) = spatial(f, tree.as_ref())?;
    let mut geo = Geometry::new(f, ops).map_err(geo_err)?;
    let unit = geo.g.length_unit;
    let to_yup = |m: &M4| mul(&YUP, &mul(&scaling(unit, unit, unit), m));

    let main = geo.products().map_err(geo_err)?;
    // ensureC: the first product with pieces moves its middle to the origin
    let mut c = I4;
    for &p in &main {
        let pcs = geo.pieces(p).map_err(geo_err)?;
        let Some(first) = pcs.first() else { continue };
        let Some((lo, hi)) = geo.mesh(first.geom).and_then(|m| m.bounds()) else { continue };
        let ctr = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
        let w = mul(&to_yup(&first.matrix), &translation(ctr[0], ctr[1], ctr[2]));
        c = translation(-w[12], -w[13], -w[14]);
        break;
    }

    let mut batches: Vec<Acc> = Vec::new();
    let mut batch_at: HashMap<String, usize> = HashMap::new();
    let mut color_keys: HashMap<[u64; 4], String> = HashMap::new();
    let mut class_of: OMap<u64, String> = OMap::new();
    let mut class_ids: Vec<Id> = Vec::new();
    let mut type_counts: OMap<String, usize> = OMap::new();
    let mut triangles = 0.0f64;
    let spaces = space_products(f)?;
    for id in main.iter().chain(spaces.iter()).copied() {
        let pcs = geo.pieces(id).map_err(geo_err)?;
        if pcs.is_empty() {
            continue;
        }
        let k = id_key(id);
        let cls = match class_of.get(&k) {
            Some(c) => c.clone(),
            None => {
                let cls = class_name(f, id);
                class_of.set(k, cls.clone());
                class_ids.push(id);
                if let Some(n) = type_counts.entry(cls.clone(), || 0) {
                    *n = n.saturating_add(1);
                }
                cls
            }
        };
        let space = cls == "IfcSpace";
        let storey = storey_of.get(&k).copied().unwrap_or(OTHER);
        let key = key_base + id;
        for p in &pcs {
            let Some(mesh) = geo.mesh(p.geom) else { continue };
            let col = p.color;
            let ck = color_keys
                .entry(col.map(f64::to_bits))
                .or_insert_with(|| col.iter().map(|v| to_fixed(*v, 3)).collect::<Vec<_>>().join(","))
                .clone();
            let bkey = format!("{}{}{}", num_str(storey), if space { "|s|" } else { "|e|" }, ck);
            let bi = match batch_at.get(&bkey) {
                Some(&i) => i,
                None => {
                    batches.push(Acc { b: Batch { storey, space, color: col, ..Batch::default() }, vcount: 0.0, tri_count: 0 });
                    let i = batches.len().saturating_sub(1);
                    batch_at.insert(bkey, i);
                    i
                }
            };
            let m = mul(&c, &to_yup(&p.matrix));
            if let Some(acc) = batches.get_mut(bi) {
                append(acc, mesh, &m, key);
            }
            triangles += usize_f64(mesh.idx.len()) / 3.0;
        }
    }
    let elements = class_of.vals.iter().filter(|c| c.as_str() != "IfcSpace").count();
    let spaces_n = class_of.vals.len().saturating_sub(elements);
    let mut types: Vec<(String, usize)> = type_counts.keys.iter().cloned().zip(type_counts.vals.iter().copied()).collect();
    types.sort_by_key(|t| std::cmp::Reverse(t.1));
    Ok(Model {
        key_base,
        schema: f.schema_name.to_string(),
        storeys,
        storey_of,
        class_of,
        class_ids,
        tree,
        batches: batches.into_iter().map(|a| a.b).collect(),
        elements,
        spaces: spaces_n,
        triangles,
        types,
        coord: c,
    })
}

/// `appendGeometry(b, verts, idx, m, key)` with the mesh's interleaved Float32 vertices.
fn append(acc: &mut Acc, mesh: &cc_io_ifc::geo::mesh::Mesh, m: &M4, key: f64) {
    let b = &mut acc.b;
    let nv = mesh.pos.len() / 3;
    let ntri = mesh.idx.len() / 3;
    let k = u32_of(key);
    let base = acc.vcount;
    let r = &mut b.elems;
    let last_same = r.ids.last().is_some_and(|&l| l == k) && r.ids.len() == r.tn.len();
    // tn / vn are JS numbers stored into a Uint32Array: counts here stay far below 2^32
    let nt32 = u32::try_from(ntri).unwrap_or(u32::MAX);
    let nv32 = u32::try_from(nv).unwrap_or(u32::MAX);
    if last_same {
        if let Some(t) = r.tn.last_mut() {
            *t = t.wrapping_add(nt32);
        }
        if let Some(v) = r.vn.last_mut() {
            *v = v.wrapping_add(nv32);
        }
    } else {
        r.ids.push(k);
        r.t0.push(u32::try_from(acc.tri_count).unwrap_or(u32::MAX));
        r.tn.push(nt32);
        r.v0.push(u32_of(base));
        r.vn.push(nv32);
    }
    b.positions.reserve(nv.saturating_mul(3));
    b.normals.reserve(nv.saturating_mul(3));
    for (p, n) in mesh.pos.as_chunks::<3>().0.iter().zip(mesh.nrm.as_chunks::<3>().0.iter()) {
        // the interleaved Float32Array the engine hands out
        let (x, y, z) = (f64::from(f32_of(p[0])), f64::from(f32_of(p[1])), f64::from(f32_of(p[2])));
        b.positions.push(f32_of(m[0] * x + m[4] * y + m[8] * z + m[12]));
        b.positions.push(f32_of(m[1] * x + m[5] * y + m[9] * z + m[13]));
        b.positions.push(f32_of(m[2] * x + m[6] * y + m[10] * z + m[14]));
        let (nx, ny, nz) = (f64::from(f32_of(n[0])), f64::from(f32_of(n[1])), f64::from(f32_of(n[2])));
        let tx = m[0] * nx + m[4] * ny + m[8] * nz;
        let ty = m[1] * nx + m[5] * ny + m[9] * nz;
        let tz = m[2] * nx + m[6] * ny + m[10] * nz;
        let l = hypot3(tx, ty, tz);
        let l = if l == 0.0 || l.is_nan() { 1.0 } else { l };
        b.normals.push(f32_of(tx / l));
        b.normals.push(f32_of(ty / l));
        b.normals.push(f32_of(tz / l));
    }
    b.indices.reserve(mesh.idx.len());
    for &i in &mesh.idx {
        b.indices.push(u32_of(base + f64::from(i)));
    }
    for _ in 0..ntri {
        b.tri_ids.push(k);
    }
    acc.tri_count = acc.tri_count.saturating_add(ntri);
    acc.vcount = base + usize_f64(nv);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uint32_wraps() {
        assert_eq!(u32_of(4_294_967_297.0), 1);
        assert_eq!(u32_of(-1.0), u32::MAX);
        assert_eq!(u32_of(7.0 * KEY_SPAN + 5.0), 469_762_053);
    }
}
