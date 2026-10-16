//! skp-import.js — a SketchUp model read by cc-skp → ModelCraft definitions: `importSKP(model, skp, name)`.
//!
//! SketchUp is Z up, in inches: points × 0.0254 → metres, placements keep their turn and scale, their move × 0.0254.
//! Components stay components, groups stay groups. A material painted on a placement becomes that placement's paint;
//! a textured material on a face comes in with its texture (the image into Model.images, the face's paint fitted to
//! the UVs). Tags on groups and components become ModelCraft tags (hidden as in the file); hidden faces, edges and
//! placements come in hidden.
//!
//! This crate may not depend on cc-io-skp (both are layer 3), so the model comes in as [`SkpModel`] — the same shape
//! as cc-skp's `readSkp` result — which `cadcraft-cli` fills from cc-io-skp, or [`SkpModel::from_json`] reads from the
//! JSON form the tests write by hand.

use std::collections::HashSet;

use cc_m3d::Model;
use cc_m3d::meshin::{PolyIn, mesh_def_in};
use cc_m3d::model::{Tag, place};
use cc_m3d::omap::FxMap;
use cc_m3d::paint::{image_paint_fit, to_base64};
use cc_m3d::vec::P3;
use serde_json::{Map, Value};

use crate::Res;
use crate::i18n::{err, t};
use crate::jsx::{get, truthy};
use crate::obj::Imported;
use crate::util::skip;

const IN: f64 = 0.0254;

/// A material: name, colour (r, g, b numbers), texture.
#[derive(Clone, Debug, Default)]
pub struct SkpMaterial {
    pub name: Option<String>,
    pub color: Option<Vec<f64>>,
    pub texture: Option<SkpTexture>,
}
/// A texture: name, MIME type, the image bytes.
#[derive(Clone, Debug, Default)]
pub struct SkpTexture {
    pub name: Option<String>,
    pub mime: Option<String>,
    pub bytes: Option<Vec<u8>>,
}
/// A tag (SketchUp layer).
#[derive(Clone, Debug, Default)]
pub struct SkpLayer {
    pub name: Option<String>,
    /// `visible === false` hides it
    pub visible: Option<bool>,
}
/// A face: loops (outer first, inches), front material, tag, UVs per loop, hidden.
#[derive(Clone, Debug, Default)]
pub struct SkpFace {
    pub loops: Vec<Vec<P3>>,
    pub front: Option<usize>,
    pub layer: Option<usize>,
    pub uv: Option<Vec<Vec<[f64; 2]>>>,
    pub hidden: bool,
}
/// A loose edge.
#[derive(Clone, Debug, Default)]
pub struct SkpEdge {
    pub a: P3,
    pub b: P3,
    pub layer: Option<usize>,
    pub hidden: bool,
}
/// A placement of another definition.
#[derive(Clone, Debug)]
pub struct SkpInst {
    pub def: usize,
    pub m: [f64; 16],
    pub name: String,
    pub layer: Option<usize>,
    pub hidden: bool,
    pub material: Option<usize>,
}
/// A definition (the model, a component, a group or an image).
#[derive(Clone, Debug, Default)]
pub struct SkpDef {
    pub name: String,
    pub kind: String,
    pub faces: Vec<SkpFace>,
    pub edges: Vec<SkpEdge>,
    pub soft: Vec<[P3; 2]>,
    pub insts: Vec<SkpInst>,
}
/// The model `readSkp` returns, as far as importSKP reads it.
#[derive(Clone, Debug, Default)]
pub struct SkpModel {
    /// `skp.format === 'skp'`
    pub format_ok: bool,
    pub version: Option<String>,
    pub materials: Vec<Option<SkpMaterial>>,
    pub layers: Vec<Option<SkpLayer>>,
    pub defs: Vec<Option<SkpDef>>,
    pub root: Option<usize>,
    pub notes: Vec<String>,
}

fn p3(v: Option<&Value>) -> P3 {
    let g = |i: usize| v.and_then(Value::as_array).and_then(|a| a.get(i)).and_then(Value::as_f64).unwrap_or(f64::NAN);
    [g(0), g(1), g(2)]
}
fn opt_idx(v: Option<&Value>) -> Option<usize> {
    v.and_then(Value::as_f64).and_then(crate::num::index)
}

impl SkpModel {
    /// The JSON form of a cc-skp model (as the tests write it by hand; texture bytes as base64 text or numbers).
    pub fn from_json(j: &Value) -> SkpModel {
        let jv = Some(j);
        let arr = |k: &str| get(jv, k).and_then(Value::as_array).cloned().unwrap_or_default();
        let materials = arr("materials")
            .iter()
            .map(|m| {
                if !truthy(Some(m)) {
                    return None;
                }
                let mv = Some(m);
                let texture = get(mv, "texture").filter(|x| truthy(Some(x))).map(|tx| {
                    let tv = Some(tx);
                    let bytes = match get(tv, "bytes") {
                        Some(Value::String(s)) => Some(crate::jsx::base64_node(s)),
                        Some(Value::Array(a)) => {
                            Some(a.iter().map(|x| x.as_u64().and_then(|n| u8::try_from(n & 255).ok()).unwrap_or(0)).collect())
                        }
                        _ => None,
                    };
                    SkpTexture {
                        name: get(tv, "name").and_then(Value::as_str).map(str::to_string),
                        mime: get(tv, "mime").and_then(Value::as_str).map(str::to_string),
                        bytes,
                    }
                });
                Some(SkpMaterial {
                    name: get(mv, "name").and_then(Value::as_str).map(str::to_string),
                    color: get(mv, "color").and_then(Value::as_array).map(|a| a.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect()),
                    texture,
                })
            })
            .collect();
        let layers = arr("layers")
            .iter()
            .map(|l| {
                truthy(Some(l)).then(|| SkpLayer {
                    name: get(Some(l), "name").and_then(Value::as_str).map(str::to_string),
                    visible: get(Some(l), "visible").and_then(Value::as_bool),
                })
            })
            .collect();
        let defs = arr("defs")
            .iter()
            .map(|d| {
                if !truthy(Some(d)) {
                    return None;
                }
                let dv = Some(d);
                let list = |k: &str| get(dv, k).and_then(Value::as_array).cloned().unwrap_or_default();
                let faces = list("faces")
                    .iter()
                    .map(|f| {
                        let fv = Some(f);
                        SkpFace {
                            loops: get(fv, "loops")
                                .and_then(Value::as_array)
                                .map(|ls| {
                                    ls.iter()
                                        .map(|l| l.as_array().map(|ps| ps.iter().map(|p| p3(Some(p))).collect()).unwrap_or_default())
                                        .collect()
                                })
                                .unwrap_or_default(),
                            front: opt_idx(get(fv, "front")),
                            layer: opt_idx(get(fv, "layer")),
                            uv: get(fv, "uv").and_then(Value::as_array).map(|ls| {
                                ls.iter()
                                    .map(|l| {
                                        l.as_array()
                                            .map(|ps| {
                                                ps.iter()
                                                    .map(|p| {
                                                        let g = |i: usize| {
                                                            p.as_array().and_then(|a| a.get(i)).and_then(Value::as_f64).unwrap_or(f64::NAN)
                                                        };
                                                        [g(0), g(1)]
                                                    })
                                                    .collect()
                                            })
                                            .unwrap_or_default()
                                    })
                                    .collect()
                            }),
                            hidden: truthy(get(fv, "hidden")),
                        }
                    })
                    .collect();
                let edges = list("edges")
                    .iter()
                    .map(|e| SkpEdge {
                        a: p3(get(Some(e), "a")),
                        b: p3(get(Some(e), "b")),
                        layer: opt_idx(get(Some(e), "layer")),
                        hidden: truthy(get(Some(e), "hidden")),
                    })
                    .collect();
                let soft = list("soft")
                    .iter()
                    .map(|s| [p3(s.as_array().and_then(|a| a.first())), p3(s.as_array().and_then(|a| a.get(1)))])
                    .collect();
                let insts = list("insts")
                    .iter()
                    .filter_map(|n| {
                        let nv = Some(n);
                        let mut m = [0.0; 16];
                        if let Some(a) = get(nv, "m").and_then(Value::as_array) {
                            for (s, x) in m.iter_mut().zip(a) {
                                *s = x.as_f64().unwrap_or(f64::NAN);
                            }
                        }
                        Some(SkpInst {
                            def: opt_idx(get(nv, "def"))?,
                            m,
                            name: get(nv, "name").and_then(Value::as_str).unwrap_or("").to_string(),
                            layer: opt_idx(get(nv, "layer")),
                            hidden: truthy(get(nv, "hidden")),
                            material: opt_idx(get(nv, "material")),
                        })
                    })
                    .collect();
                Some(SkpDef {
                    name: get(dv, "name").and_then(Value::as_str).unwrap_or("").to_string(),
                    kind: get(dv, "kind").and_then(Value::as_str).unwrap_or("").to_string(),
                    faces,
                    edges,
                    soft,
                    insts,
                })
            })
            .collect();
        SkpModel {
            format_ok: get(jv, "format").and_then(Value::as_str) == Some("skp"),
            version: get(jv, "version").and_then(Value::as_str).map(str::to_string),
            materials,
            layers,
            defs,
            root: opt_idx(get(jv, "root")),
            notes: arr("notes").iter().filter_map(|n| n.as_str().map(str::to_string)).collect(),
        }
    }
}

fn to_m(p: P3) -> P3 {
    [p[0] * IN, p[1] * IN, p[2] * IN]
}
/// `pkey(p)`: the JS key is `String(Math.round(x * 1e5))` per axis; String is one-to-one on numbers except that
/// −0 and 0 both read "0" (and every NaN reads "NaN"), so the rounded numbers with those folded make the same key.
type PKey = [u64; 3];
fn pkey(p: P3) -> PKey {
    p.map(|x| {
        let r = cc_m3d::js::round(x * 1e5);
        if r == 0.0 {
            0
        } else if r.is_nan() {
            f64::NAN.to_bits()
        } else {
            r.to_bits()
        }
    })
}
/// `ekey(a, b)`: the two point keys in a fixed order (the JS sorts the strings; only set membership is used).
fn ekey(a: P3, b: P3) -> (PKey, PKey) {
    let (x, y) = (pkey(a), pkey(b));
    if x <= y { (x, y) } else { (y, x) }
}
fn hex(c: &[f64]) -> String {
    let mut s = String::from("#");
    for v in c {
        let h = crate::num::hex((0.0f64).max((255.0f64).min(cc_m3d::js::round(*v))));
        if h.len() < 2 {
            s.push('0');
        }
        s.push_str(&h);
    }
    s
}
/// `clean(s, dflt)`: control characters → spaces, trimmed, at most 80 UTF-16 units, else dflt.
fn clean(s: Option<&str>, dflt: &str) -> String {
    let c = cc_m3d::js::clean_text(s.unwrap_or(""), 80);
    if c.is_empty() { dflt.to_string() } else { c }
}
fn newell(l: &[P3]) -> P3 {
    let mut n = [0.0; 3];
    let k = l.len();
    for (i, p) in l.iter().enumerate() {
        let q = l.get(i.saturating_add(1).checked_rem(k).unwrap_or(0)).copied().unwrap_or(*p);
        n[0] += (p[1] - q[1]) * (p[2] + q[2]);
        n[1] += (p[2] - q[2]) * (p[0] + q[0]);
        n[2] += (p[0] - q[0]) * (p[1] + q[1]);
    }
    n
}

struct Imp<'s> {
    skp: &'s SkpModel,
    color_of: Vec<Option<String>>,
    textured: Vec<bool>,
    layer_off: Vec<bool>,
    image_ids: FxMap<usize, Option<String>>,
    tag_names: FxMap<usize, Option<String>>,
    built: FxMap<usize, Option<String>>,
    building: HashSet<usize>,
    made: Vec<String>,
    skipped: Map<String, Value>,
    rep: [f64; 9],
    textured_kept: Option<f64>,
}
// rep: faces, edges, components, groups, textured, hidden, tags, untagged
const R_TEXTURED: usize = 4;
const R_HIDDEN: usize = 5;
const R_TAGS: usize = 6;
const R_UNTAGGED: usize = 7;

impl Imp<'_> {
    fn inc(&mut self, k: usize) {
        if let Some(r) = self.rep.get_mut(k) {
            *r += 1.0;
        }
    }
    fn off(&self, layer: Option<usize>) -> bool {
        layer.is_some_and(|l| self.layer_off.get(l).copied().unwrap_or(false))
    }
    fn tagged(&self, layer: Option<usize>) -> bool {
        layer.is_some_and(|l| l > 0 && self.skp.layers.get(l).is_some_and(Option::is_some))
    }
    /// `imageOf(mat)`: the material's texture kept in model.images (once).
    fn image_of(&mut self, model: &mut Model, mat: usize) -> Option<String> {
        if let Some(id) = self.image_ids.get(&mat) {
            return id.clone();
        }
        let m = self.skp.materials.get(mat).and_then(Option::as_ref);
        let mut id = None;
        if let Some(m) = m
            && let Some(tx) = &m.texture
            && let Some(bytes) = &tx.bytes
        {
            let mime = tx.mime.as_deref().unwrap_or("");
            if matches!(mime, "image/png" | "image/jpeg" | "image/webp") {
                let nm_src = tx.name.as_deref().filter(|s| !s.is_empty()).or(m.name.as_deref());
                let avg = self.color_of.get(mat).cloned().flatten();
                id = model.add_image(&clean(nm_src, "texture"), mime, &to_base64(bytes), avg.as_deref()).ok();
            }
        }
        self.image_ids.insert(mat, id.clone());
        id
    }
    /// `tagFor(layer)`: a SketchUp tag index → a ModelCraft tag name (made on first use).
    fn tag_for(&mut self, model: &mut Model, layer: Option<usize>) -> Option<String> {
        if !self.tagged(layer) {
            return None;
        }
        let l = layer?;
        if let Some(t) = self.tag_names.get(&l) {
            return t.clone();
        }
        let nm = clean(self.skp.layers.get(l).and_then(Option::as_ref).and_then(|x| x.name.as_deref()), "");
        let mut tag = None;
        if !nm.is_empty() && model.tags.len() < 1000 {
            match model.tag(&nm) {
                Some(i) => tag = model.tags.get(i).map(|g| g.name.clone()),
                None => {
                    let hidden = self.layer_off.get(l).copied().unwrap_or(false);
                    model.tags.push(Tag { name: nm.clone(), hidden });
                    tag = Some(nm);
                    self.inc(R_TAGS);
                }
            }
        }
        self.tag_names.insert(l, tag.clone());
        tag
    }

    #[allow(clippy::too_many_lines)]
    fn build(&mut self, model: &mut Model, i: usize, depth: u32) -> Option<String> {
        let skp = self.skp;
        let d = skp.defs.get(i).and_then(Option::as_ref)?;
        if let Some(b) = self.built.get(&i) {
            return b.clone();
        }
        if depth > 64 || self.building.contains(&i) {
            skip(&mut self.skipped, &t("app.import.skp.skip.nested", &[]), 1.0);
            return None;
        }
        if d.kind == "image" {
            skip(&mut self.skipped, &t("app.import.skp.skip.images", &[]), 1.0);
            self.built.insert(i, None);
            return None;
        }
        self.building.insert(i);
        let mut shown: Vec<PolyIn> = Vec::new();
        let mut hidden_polys: Vec<PolyIn> = Vec::new();
        for f in &d.faces {
            let mat = f.front;
            let is_tex = mat.is_some_and(|m| self.textured.get(m).copied().unwrap_or(false));
            if is_tex {
                self.inc(R_TEXTURED);
            }
            let hide = f.hidden || self.off(f.layer);
            if !hide && self.tagged(f.layer) {
                self.inc(R_UNTAGGED);
            }
            let loops: Vec<Vec<P3>> = f.loops.iter().map(|l| l.iter().map(|&p| to_m(p)).collect()).collect();
            let mut color = mat.and_then(|m| self.color_of.get(m).cloned().flatten());
            if let (Some(m), true) = (mat, is_tex)
                && let Some(uv0) = f.uv.as_ref().and_then(|u| u.first())
            {
                let id = self.image_of(model, m);
                if let Some(id) = id {
                    let l0 = loops.first().cloned().unwrap_or_default();
                    let avg = self.color_of.get(m).cloned().flatten();
                    if let Some(fit) = image_paint_fit(&id, &l0, uv0, newell(&l0), avg.as_deref()) {
                        color = Some(fit);
                        self.textured_kept = Some(self.textured_kept.unwrap_or(0.0) + 1.0);
                    }
                }
            }
            let p = PolyIn { loops, color };
            if hide {
                hidden_polys.push(p);
            } else {
                shown.push(p);
            }
        }
        let mut segs: Vec<[P3; 2]> = Vec::new();
        let mut hidden_segs: HashSet<(PKey, PKey)> = HashSet::new();
        for e in &d.edges {
            let (a, b) = (to_m(e.a), to_m(e.b));
            segs.push([a, b]);
            if e.hidden || self.off(e.layer) {
                hidden_segs.insert(ekey(a, b));
            } else if self.tagged(e.layer) {
                self.inc(R_UNTAGGED);
            }
        }
        struct Place {
            id: String,
            m: [f64; 16],
            name: String,
            hidden: bool,
            tag: Option<String>,
            color: Option<String>,
        }
        let mut places: Vec<Place> = Vec::new();
        for n in &d.insts {
            let Some(id) = self.build(model, n.def, depth.saturating_add(1)) else { continue };
            let mut m = n.m;
            m[12] *= IN;
            m[13] *= IN;
            m[14] *= IN;
            let tag = self.tag_for(model, n.layer);
            places.push(Place {
                id,
                m,
                name: n.name.clone(),
                hidden: n.hidden,
                tag,
                color: n.material.and_then(|mt| self.color_of.get(mt).cloned().flatten()),
            });
        }
        self.building.remove(&i);
        let mut id = None;
        if !shown.is_empty() || !hidden_polys.is_empty() || !segs.is_empty() || !places.is_empty() {
            let kind = if d.kind == "component" { "component" } else { "group" };
            // faces go in the order given, so the hidden ones are the faces after those the shown polygons make
            let n_shown = if hidden_polys.is_empty() {
                0
            } else {
                let mut scratch = Model::new();
                let sid = mesh_def_in(&mut scratch, &shown, Some("count"), "group", &[]);
                scratch.def(&sid).map_or(0, |x| x.faces.len())
            };
            let dflt = if kind == "component" { t("app.import.skp.component", &[]) } else { t("app.import.skp.group", &[]) };
            let nhid = hidden_polys.len();
            let mut all = shown;
            all.extend(hidden_polys);
            let did = mesh_def_in(model, &all, Some(&clean(Some(&d.name), &dflt)), kind, &segs);
            let mut nh = 0.0;
            if let Some(def) = model.def_mut(&did) {
                if nhid > 0 {
                    for (k, f) in def.faces.values_mut().enumerate() {
                        if k >= n_shown {
                            f.hidden = true;
                            nh += 1.0;
                        }
                    }
                }
                let soft: Option<HashSet<(PKey, PKey)>> =
                    (!d.soft.is_empty()).then(|| d.soft.iter().map(|[a, b]| ekey(to_m(*a), to_m(*b))).collect());
                if soft.is_some() || !hidden_segs.is_empty() {
                    let verts = def.verts.clone();
                    for e in def.edges.values_mut() {
                        let k = ekey(verts.get(e.a).copied().unwrap_or([f64::NAN; 3]), verts.get(e.b).copied().unwrap_or([f64::NAN; 3]));
                        if soft.as_ref().is_some_and(|s| s.contains(&k)) {
                            e.soft = true;
                        }
                        if hidden_segs.contains(&k) {
                            e.hidden = true;
                            nh += 1.0;
                        }
                    }
                }
                for p in places {
                    let iid = place(def, &p.id, &p.m);
                    if let Some(inst) = def.insts.get_mut(iid) {
                        if !p.name.is_empty() {
                            inst.name = Some(clean(Some(&p.name), ""));
                        }
                        if let Some(c) = p.color.filter(|c| !c.is_empty()) {
                            inst.color = Some(c);
                        }
                        if let Some(tg) = p.tag.filter(|c| !c.is_empty()) {
                            inst.tag = Some(tg);
                        }
                        if p.hidden {
                            inst.hidden = true;
                            nh += 1.0;
                        }
                    }
                }
            }
            if let Some(r) = self.rep.get_mut(R_HIDDEN) {
                *r += nh;
            }
            self.made.push(did.clone());
            id = Some(did);
        }
        self.built.insert(i, id.clone());
        id
    }
}

/// `importSKP(model, skp, name)` → {def, report}: a new group, not placed yet.
pub fn import_skp(model: &mut Model, skp: &SkpModel, name: &str) -> Res<Imported> {
    let root_ok = skp.root.and_then(|r| skp.defs.get(r)).is_some_and(Option::is_some);
    if !skp.format_ok || !root_ok {
        return Err(err("app.import.skp.bad"));
    }
    let mut imp = Imp {
        skp,
        color_of: skp.materials.iter().map(|m| m.as_ref().and_then(|m| m.color.as_ref()).map(|c| hex(c))).collect(),
        textured: skp.materials.iter().map(|m| m.as_ref().is_some_and(|m| m.texture.is_some())).collect(),
        layer_off: skp.layers.iter().map(|l| l.as_ref().is_some_and(|l| l.visible == Some(false))).collect(),
        image_ids: FxMap::default(),
        tag_names: FxMap::default(),
        built: FxMap::default(),
        building: HashSet::new(),
        made: Vec::new(),
        skipped: Map::new(),
        rep: [0.0; 9],
        textured_kept: None,
    };
    let root = skp.root.unwrap_or(0);
    let Some(root_id) = imp.build(model, root, 0) else {
        for d in &imp.made {
            model.defs.delete(d);
        }
        return Err(err("app.import.skp.empty"));
    };
    if let Some(top) = model.def_mut(&root_id) {
        top.kind = "group".into();
        top.name = clean(Some(name), &t("app.import.default", &[]));
    }
    let (mut faces, mut edges, mut comps, mut groups) = (0.0, 0.0, 0.0, 0.0);
    for id in &imp.made {
        let Some(d) = model.def(id) else { continue };
        faces += d.faces.len() as f64;
        edges += d.edges.len() as f64;
        if *id == root_id {
            continue;
        }
        if d.kind == "component" {
            comps += 1.0;
        } else {
            groups += 1.0;
        }
    }
    let mut report = Map::new();
    if let Some(v) = &skp.version {
        report.insert("version".into(), Value::String(v.clone()));
    }
    report.insert("faces".into(), Value::from(faces));
    report.insert("edges".into(), Value::from(edges));
    report.insert("components".into(), Value::from(comps));
    report.insert("groups".into(), Value::from(groups));
    report.insert("textured".into(), Value::from(imp.rep.get(R_TEXTURED).copied().unwrap_or(0.0)));
    report.insert("hidden".into(), Value::from(imp.rep.get(R_HIDDEN).copied().unwrap_or(0.0)));
    report.insert("tags".into(), Value::from(imp.rep.get(R_TAGS).copied().unwrap_or(0.0)));
    report.insert("untagged".into(), Value::from(imp.rep.get(R_UNTAGGED).copied().unwrap_or(0.0)));
    report.insert("skipped".into(), Value::Object(imp.skipped));
    report.insert("notes".into(), Value::Array(skp.notes.iter().map(|n| Value::String(n.clone())).collect()));
    if let Some(k) = imp.textured_kept {
        report.insert("texturedKept".into(), Value::from(k));
    }
    Ok(Imported { def: root_id, report, units: None })
}
