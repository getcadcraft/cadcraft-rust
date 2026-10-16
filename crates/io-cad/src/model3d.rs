//! model3d.js, the reading side: 3DFACEs (`face3dFromDwg`), CadCraft's 3D JSON and view from block CADCRAFT_3D
//! (`read3dFromDwg`, `read3dText`, `read3dView`, `cleanView`) and `split3d` (3D faces out of the 2D drawing). The 3D
//! model itself (Model3D) is not ported: its JSON and faces pass through as data.

use cc_cad::js;
use cc_cad_geom::jsmath::JsMath;
use cc_io_dwg::value::{Obj, V};

use crate::i18n::{self, Catalog};
use crate::jsv::{self, Ov};
use crate::small::{cadcraft_string, hypot3, read3d_text, round_precision};

pub const M3D_BLOCK: &str = "CADCRAFT_3D";
pub const M3D_LAYER: &str = "CC-3D";
/// 3D faces placed by references: the rest are left out.
const MAX_PLACED: usize = 200_000;
/// Inserts visited while placing faces (the JS has no cap: a hostile nesting would never end).
const MAX_VISITS: usize = 4_000_000;

/// `isM3dBlock(name)`
pub fn is_m3d_block(name: Ov) -> bool {
    jsv::upper(&jsv::str_or_empty(name)) == M3D_BLOCK
}

/// STYLES keys, plus what `STYLES[name]` finds on Object.prototype (the JS test is a plain property read).
fn is_style(s: &str) -> bool {
    matches!(
        s,
        "shaded"
            | "edges"
            | "hidden"
            | "wire"
            | "xray"
            | "constructor"
            | "__defineGetter__"
            | "__defineSetter__"
            | "hasOwnProperty"
            | "__lookupGetter__"
            | "__lookupSetter__"
            | "isPrototypeOf"
            | "propertyIsEnumerable"
            | "toString"
            | "valueOf"
            | "__proto__"
            | "toLocaleString"
    )
}

/// `cleanView(v)`: a saved 3D view (untrusted) → { eye, target, persp, fov, style? } or None.
pub fn clean_view(v: Ov) -> Option<V> {
    let o = jsv::obj(v)?;
    let p = |x: Ov| -> Option<[f64; 3]> {
        let a = match x {
            Some(V::Arr(a)) if a.len() == 3 => a,
            _ => return None,
        };
        let mut out = [0.0; 3];
        for (slot, q) in out.iter_mut().zip(a) {
            let f = jsv::fin(Some(q)).filter(|f| f.abs() < 1e9)?;
            *slot = round_precision(f, 12);
        }
        Some(out)
    };
    let eye = p(o.get("eye"))?;
    let target = p(o.get("target"))?;
    if hypot3(eye[0] - target[0], eye[1] - target[1], eye[2] - target[2]) < 1e-9 {
        return None;
    }
    let fov = jsv::fin(o.get("fov")).filter(|f| (5.0..=120.0).contains(f)).unwrap_or(35.0);
    let mut out = Obj::new();
    out.set("eye", V::Arr(eye.iter().map(|x| V::Num(*x)).collect()));
    out.set("target", V::Arr(target.iter().map(|x| V::Num(*x)).collect()));
    out.set("persp", !matches!(o.get("persp"), Some(V::Bool(false))));
    out.set("fov", fov);
    if let Some(st) = jsv::s(o.get("style")).filter(|s| is_style(s)) {
        out.set("style", st);
    }
    Some(V::Obj(out))
}

/// `read3dView(ents)`: the 3D view saved beside the model (a point "CCCAM:{json}"), or None.
pub fn read3d_view(ents: &[V]) -> Option<V> {
    for e in ents {
        let cc = jsv::get(Some(e), "cc");
        let s = if matches!(cc, Some(V::Arr(_))) { jsv::str_nn(jsv::at(cc, 0)) } else { String::new() };
        if s.starts_with("CCCAM:") && crate::text::len16(&s) < 2000 {
            return match serde_json::from_str::<serde_json::Value>(s.get(6..).unwrap_or("")) {
                Ok(j) => clean_view(Some(&jsv::from_json(&j))),
                Err(_) => None,
            };
        }
    }
    None
}

/// `face3dFromDwg(e, base)`: a DWG 3DFACE → { type: 'face3d', ...base, pts: 4 × [x, y, z] }, or None.
pub fn face3d_from_dwg(e: &Obj, base: &Obj) -> Option<V> {
    let p = jsv::arr(e.get("points"));
    let pick = [p.first(), p.get(1), p.get(2), p.get(3).filter(|q| q.truthy()).or(p.get(2))];
    let c: Vec<&V> = pick
        .into_iter()
        .flatten()
        .filter(|q| q.truthy() && jsv::fin(jsv::get(Some(q), "x")).is_some() && jsv::fin(jsv::get(Some(q), "y")).is_some())
        .collect();
    if c.len() < 3 {
        return None;
    }
    let mut o = Obj::new();
    o.set("type", "face3d");
    o.assign(base.clone());
    o.set(
        "pts",
        V::Arr(
            c.iter()
                .map(|q| {
                    let g = |k: &str| jsv::get(Some(q), k);
                    V::Arr(vec![V::Num(jsv::to_num(g("x"))), V::Num(jsv::to_num(g("y"))), V::Num(jsv::fin(g("z")).unwrap_or(0.0))])
                })
                .collect(),
        ),
    );
    Some(V::Obj(o))
}

fn insert_matrix2(e: &V, b: &V) -> [f64; 6] {
    let g = |k: &str| jsv::get(Some(e), k);
    let rot = jsv::or(g("rot"), 0.0);
    let (c, s) = (rot.jcos(), rot.jsin());
    let sx = jsv::nn(g("sx"), 1.0);
    let sy = jsv::nn(g("sy"), 1.0);
    let base = jsv::get(Some(b), "base");
    let bx = jsv::or(jsv::get(base, "x"), 0.0);
    let by = jsv::or(jsv::get(base, "y"), 0.0);
    let (a, bb, cc, d) = (c * sx, s * sx, -s * sy, c * sy);
    [a, bb, cc, d, jsv::or(g("x"), 0.0) - (a * bx + cc * by), jsv::or(g("y"), 0.0) - (bb * bx + d * by)]
}
fn mul2(m: &[f64; 6], n: &[f64; 6]) -> [f64; 6] {
    [
        m[0] * n[0] + m[2] * n[1],
        m[1] * n[0] + m[3] * n[1],
        m[0] * n[2] + m[2] * n[3],
        m[1] * n[2] + m[3] * n[3],
        m[0] * n[4] + m[2] * n[5] + m[4],
        m[1] * n[4] + m[3] * n[5] + m[5],
    ]
}

fn is_face(e: &V) -> bool {
    jsv::s(jsv::get(Some(e), "type")) == Some("face3d")
}

struct Walk<'a> {
    by_name: std::collections::HashMap<String, &'a V>,
    own: std::collections::HashMap<String, Vec<&'a V>>,
    visits: usize,
}

impl Walk<'_> {
    fn walk(&mut self, list: &[V], m: &[f64; 6], depth: u32, out: &mut Vec<V>) {
        if depth > 16 || out.len() > MAX_PLACED || self.visits > MAX_VISITS {
            return;
        }
        for e in list {
            if jsv::s(jsv::get(Some(e), "type")) != Some("insert") {
                continue;
            }
            self.visits = self.visits.saturating_add(1);
            if self.visits > MAX_VISITS {
                return;
            }
            let name = jsv::to_str(jsv::get(Some(e), "block"));
            let Some(b) = self.by_name.get(&name).copied() else { continue };
            let mm = mul2(m, &insert_matrix2(e, b));
            let bname = jsv::to_str(jsv::get(Some(b), "name"));
            for f in self.own.get(&bname).cloned().unwrap_or_default() {
                let Some(fo) = jsv::obj(Some(f)) else { continue };
                let mut c = fo.clone();
                let fl = fo.get("layer");
                let layer = if jsv::s(fl) == Some("0") { Some(jsv::orv(jsv::get(Some(e), "layer"), V::from("0"))) } else { fl.cloned() };
                c.set_opt("layer", layer);
                let pts: Vec<V> = jsv::arr(fo.get("pts"))
                    .iter()
                    .map(|q| {
                        let n = |i: usize| jsv::to_num(jsv::at(Some(q), i));
                        V::Arr(vec![
                            V::Num(mm[0] * n(0) + mm[2] * n(1) + mm[4]),
                            V::Num(mm[1] * n(0) + mm[3] * n(1) + mm[5]),
                            V::Num(jsv::or(jsv::at(Some(q), 2), 0.0)),
                        ])
                    })
                    .collect();
                c.set("pts", V::Arr(pts));
                out.push(V::Obj(c));
            }
            let ents = jsv::arr(jsv::get(Some(b), "entities"));
            self.walk(ents, &mm, depth.saturating_add(1), out);
        }
    }
}

/// face3d → a flat closed outline (poly), or None (dropped).
fn outline(e: &Obj) -> Option<V> {
    let mut pts: Vec<(f64, f64)> = Vec::new();
    for q in jsv::arr(e.get("pts")) {
        let x = jsv::to_num(jsv::at(Some(q), 0));
        let y = jsv::to_num(jsv::at(Some(q), 1));
        let far = match pts.last() {
            None => true,
            Some(l) => js::hypot(x - l.0, y - l.1) > 1e-12,
        };
        if far {
            pts.push((x, y));
        }
    }
    if pts.len() > 1
        && let (Some(f), Some(l)) = (pts.first(), pts.last())
        && js::hypot(f.0 - l.0, f.1 - l.1) <= 1e-12
    {
        pts.pop();
    }
    if pts.len() < 2 {
        return None;
    }
    let mut o = Obj::new();
    o.set("type", "poly");
    o.set("layer", jsv::orv(e.get("layer"), V::from("0")));
    o.set("color", jsv::nnv(e.get("color"), V::Null));
    o.set("closed", pts.len() > 2);
    o.set("pts", V::Arr(pts.iter().map(|(x, y)| V::Obj(cc_io_dwg::obj! { "x" => *x, "y" => *y, "b" => 0.0 })).collect()));
    Some(V::Obj(o))
}

fn to_outlines(list: &mut Vec<V>) -> usize {
    let mut flat = 0usize;
    let mut out = Vec::with_capacity(list.len());
    for e in std::mem::take(list) {
        if !is_face(&e) {
            out.push(e);
            continue;
        }
        flat = flat.saturating_add(1);
        if let Some(p) = jsv::obj(Some(&e)).and_then(outline) {
            out.push(p);
        }
    }
    *list = out;
    flat
}

/// `split3d(input)`: 3DFACEs out of a reader's result, in place (model ones into `faces3d`, block ones placed by every
/// model-space reference; in blocks and on sheets they become flat outlines).
pub fn split3d(input: &mut Obj, cat: &Catalog) {
    let mut faces: Vec<V> = match input.get("faces3d") {
        Some(V::Arr(a)) => a.clone(),
        _ => Vec::new(),
    };
    if let Some(V::Arr(ents)) = input.get_mut("entities") {
        let mut kept = Vec::new();
        ents.retain(|e| {
            if is_face(e) {
                kept.push(e.clone());
                false
            } else {
                true
            }
        });
        faces.extend(kept);
    }
    let mut placed = 0usize;
    if let Some(V::Arr(blocks)) = input.get_mut("blocks") {
        blocks.retain(|b| !is_m3d_block(jsv::get(Some(b), "name")));
    }
    let empty: Vec<V> = Vec::new();
    {
        let blocks = match input.get("blocks") {
            Some(V::Arr(b)) => b,
            _ => &empty,
        };
        let mut own: std::collections::HashMap<String, Vec<&V>> = std::collections::HashMap::new();
        for b in blocks {
            let list: Vec<&V> = jsv::arr(jsv::get(Some(b), "entities")).iter().filter(|e| is_face(e)).collect();
            if !list.is_empty() {
                own.insert(jsv::to_str(jsv::get(Some(b), "name")), list);
            }
        }
        if !own.is_empty() {
            let by_name = blocks.iter().map(|b| (jsv::to_str(jsv::get(Some(b), "name")), b)).collect();
            let mut w = Walk { by_name, own, visits: 0 };
            let mut out = Vec::new();
            let ents = jsv::arr(input.get("entities"));
            w.walk(ents, &[1.0, 0.0, 0.0, 1.0, 0.0, 0.0], 0, &mut out);
            placed = out.len().min(MAX_PLACED);
            out.truncate(MAX_PLACED);
            faces.extend(out);
        }
    }
    let mut flat = 0usize;
    if let Some(V::Arr(blocks)) = input.get_mut("blocks") {
        for b in blocks.iter_mut() {
            if let V::Obj(bo) = b
                && let Some(V::Arr(list)) = bo.get_mut("entities")
            {
                flat = flat.saturating_add(to_outlines(list));
            }
        }
    }
    if let Some(V::Arr(layouts)) = input.get_mut("layouts") {
        for l in layouts.iter_mut() {
            if let V::Obj(lo) = l
                && let Some(V::Arr(list)) = lo.get_mut("entities")
            {
                flat = flat.saturating_add(to_outlines(list));
            }
        }
    }
    if let Some(V::Obj(rep)) = input.get_mut("report")
        && let Some(V::Arr(notes)) = rep.get_mut("notes")
    {
        if flat > 0 {
            notes.push(V::Str(cat.t("model3d.read.flat", &i18n::n(jsv::us2f(flat)))));
        }
        if placed > 0 {
            notes.push(V::Str(cat.t("model3d.read.placed", &i18n::n(jsv::us2f(placed)))));
        }
    }
    if !faces.is_empty() {
        input.set("faces3d", V::Arr(faces));
    }
}

/// `read3dFromDwg(db, drawing)`: CadCraft's 3D JSON from block CADCRAFT_3D → drawing.model3d (+ view3d); that block
/// leaves the drawing; then split3d.
pub fn read3d_from_dwg(db: &Obj, drawing: &mut Obj, cat: &Catalog) {
    if let Some(blk) = jsv::arr(db.get("blocks")).iter().find(|b| is_m3d_block(jsv::get(Some(b), "name"))) {
        let ents: Vec<V> = jsv::arr(jsv::get(Some(blk), "entities"))
            .iter()
            .map(|e| {
                let s = cadcraft_string(jsv::get(Some(e), "xdata")).unwrap_or_default();
                V::Obj(cc_io_dwg::obj! { "cc" => V::Arr(vec![V::Str(s)]) })
            })
            .collect();
        if let Some(text) = read3d_text(&ents) {
            drawing.set("model3d", text);
        }
        if let Some(view) = read3d_view(&ents) {
            drawing.set("view3d", view);
        }
    }
    if let Some(V::Arr(blocks)) = drawing.get_mut("blocks") {
        blocks.retain(|b| !is_m3d_block(jsv::get(Some(b), "name")));
    }
    split3d(drawing, cat);
}
