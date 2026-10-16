//! `meshDef` / `edgesDef` with typed input, for the file readers (cc-io-m3d): the same steps as [`crate::mesh`]'s
//! JSON-value versions (which serve the op-script runner), without building a JSON value per point. Points are three
//! numbers; non-finite ones stay as they are (a JSON value cannot hold them).

use crate::def::*;
use crate::js;
use crate::model::Model;
use crate::omap::FxMap;
use crate::vec::*;

/// One imported polygon: its loops (outer first, then holes) and its colour.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PolyIn {
    pub loops: Vec<Vec<P3>>,
    pub color: Option<String>,
}

impl PolyIn {
    /// A plain array of points (one outer loop, no colour).
    pub fn outline(pts: Vec<P3>) -> Self {
        Self { loops: vec![pts], color: None }
    }
}

/// `p.map(x => Math.round(x * 1e5)).join(',')`.
fn grid_key(p: &P3, s: &mut String) {
    s.clear();
    for (i, x) in p.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        js::push_num(s, js::round(x * 1e5));
    }
}

struct Grid {
    map: FxMap<String, f64>,
    key: String,
}
impl Grid {
    fn new() -> Self {
        Self { map: FxMap::default(), key: String::new() }
    }
    fn vid(&mut self, d: &mut Def, p: &P3) -> f64 {
        grid_key(p, &mut self.key);
        if let Some(&id) = self.map.get(&self.key) {
            return id;
        }
        let id = v_add(d, *p);
        self.map.insert(self.key.clone(), id);
        id
    }
}

/// `meshDef(model, polys, name, kind, segs)` → the new definition's id. Not flat: a fan of triangles. Edges between
/// coplanar faces are soft. `segs` become plain edges, welded to the faces' vertices.
pub fn mesh_def_in(model: &mut Model, polys: &[PolyIn], name: Option<&str>, kind: &str, segs: &[[P3; 2]]) -> String {
    let id = model.new_def(kind, name);
    let Some(slot) = model.defs.get_mut(&id) else { return id };
    let mut d = std::mem::replace(slot, Def::new(&id, kind, ""));
    let mut grid = Grid::new();
    let ring = |d: &mut Def, grid: &mut Grid, pts: &[P3]| -> Option<Vec<f64>> {
        let mut lp: Vec<f64> = Vec::with_capacity(pts.len());
        for p in pts {
            let v = grid.vid(d, p);
            if lp.last() != Some(&v) {
                lp.push(v);
            }
        }
        while lp.len() > 1 && lp.first() == lp.last() {
            lp.pop();
        }
        if lp.len() < 3 {
            return None;
        }
        let mut uniq = lp.clone();
        uniq.sort_by(f64::total_cmp);
        uniq.dedup();
        if uniq.len() != lp.len() {
            return None;
        }
        if poly_area_of(d, &lp) <= EPS2 { None } else { Some(lp) }
    };
    for poly in polys {
        let Some(outer) = poly.loops.first().and_then(|r| ring(&mut d, &mut grid, r)) else { continue };
        let pp: Vec<P3> = outer.iter().map(|&v| d.v(v)).collect();
        let n = newell(&pp);
        let p0 = pp.first().copied().unwrap_or([f64::NAN; 3]);
        let mut size = 0.0;
        for p in &pp {
            size = js::max(size, dist(*p, p0));
        }
        let tol = js::max(EPS, size * 1e-6);
        if pp.iter().any(|p| dot(n, sub(*p, p0)).abs() > tol) {
            // not flat: a fan of triangles (each one flat by definition)
            let mut i = 1usize;
            while i + 1 < outer.len() {
                let t = vec![
                    outer.first().copied().unwrap_or(f64::NAN),
                    outer.get(i).copied().unwrap_or(f64::NAN),
                    outer.get(i + 1).copied().unwrap_or(f64::NAN),
                ];
                if poly_area_of(&d, &t) > EPS2 {
                    f_add(&mut d, vec![t], poly.color.clone(), None, false);
                }
                i += 1;
            }
            continue;
        }
        let mut holes = Vec::new();
        for r in poly.loops.iter().skip(1) {
            let Some(mut h) = ring(&mut d, &mut grid, r) else { continue };
            if h.iter().any(|v| outer.contains(v)) {
                continue;
            }
            if dot(newell_of(&d, &h), n) > 0.0 {
                h.reverse();
            }
            holes.push(h);
        }
        let mut loops = vec![outer];
        loops.extend(holes);
        f_add(&mut d, loops, poly.color.clone(), None, false);
    }
    for [p, q] in segs {
        let a = grid.vid(&mut d, p);
        let b = grid.vid(&mut d, q);
        if a != b {
            e_add0(&mut d, a, b);
        }
    }
    let ef = edge_faces(&d);
    let mut soft = Vec::new();
    for (eid, e) in d.edges.iter() {
        if let [f0, f1] = ef_get(&ef, e.a, e.b)
            && let (Some(fa), Some(fb)) = (d.faces.get(*f0), d.faces.get(*f1))
            && dot(face_normal(&d, fa), face_normal(&d, fb)) > 1.0 - 1e-6
        {
            soft.push(eid);
        }
    }
    for eid in soft {
        if let Some(e) = d.edges.get_mut(eid) {
            e.soft = true;
        }
    }
    if let Some(slot) = model.defs.get_mut(&id) {
        *slot = d;
    }
    id
}

/// `edgesDef(model, segs, name)` → the new definition's id.
pub fn edges_def_in(model: &mut Model, segs: &[[P3; 2]], name: Option<&str>) -> String {
    let id = model.new_def("group", name);
    let Some(d) = model.defs.get_mut(&id) else { return id };
    let mut grid = Grid::new();
    for [p, q] in segs {
        let a = grid.vid(d, p);
        let b = grid.vid(d, q);
        if a != b {
            e_add0(d, a, b);
        }
    }
    id
}
