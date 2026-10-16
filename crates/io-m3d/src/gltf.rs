//! glTF 2.0: io.js `exportGLB` (binary) and `importGLTF` (.glb, or .gltf with data: URIs only).
//!
//! Export: Y up as glTF readers expect, through a root node that turns Z up into Y up; components stay shared (one
//! mesh per definition and placement colour, one node per instance); edges without faces are left out. Import: every
//! offset is checked against its buffer, accessors are read in place (no copy: a "zeros" accessor without a buffer
//! view costs nothing), node trees are visited at most 200 000 times and 64 deep.

use cc_m3d::Model;
use cc_m3d::json::Out;
use cc_m3d::meshin::{PolyIn, mesh_def_in};
use cc_m3d::omap::FxMap;
use cc_m3d::paint::{image_paint_fit, is_material_paint, to_base64};
use cc_m3d::vec::{IDENT, M4, P3, add, apply, cross, det3, mmul, mul, sub};
use serde_json::{Map, Value};

use crate::Res;
use crate::i18n::{err, err_with, t};
use crate::jsx::{arr, at_key, base64_node, get, int, num, pow, round, truthy};
use crate::num::{hex, is_integer, to_i32};
use crate::obj::{Imported, parse_hex};
use crate::stl::f32_of;
use crate::util::{MAX_VERTS, clean_name, def_tris, paint_hex, skip};

const Z2Y: M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0];
/// Most nodes an export writes: definitions placed inside each other multiply (the JS would run out of memory first).
const MAX_NODES: usize = 4_000_000;

fn is_ident_m(m: &M4) -> bool {
    m.iter().enumerate().all(|(i, v)| (v - if i % 5 == 0 { 1.0 } else { 0.0 }).abs() < 1e-12)
}

fn srgb_to_lin(c: f64) -> f64 {
    if c <= 0.04045 { c / 12.92 } else { pow((c + 0.055) / 1.055, 2.4) }
}
fn lin_to_srgb(c: f64) -> f64 {
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * pow(c, 1.0 / 2.4) - 0.055 }
}
/// `hexToLinear(hex)`: a library material exports as its colour.
fn hex_to_linear(c: &str) -> [f64; 3] {
    let h = paint_hex(c);
    let mut it = h.chars();
    it.next();
    let n = to_i32(parse_hex(it.as_str()));
    let ch = |s: i32| f64::from((n >> s) & 255) / 255.0;
    [srgb_to_lin(ch(16)), srgb_to_lin(ch(8)), srgb_to_lin(ch(0))]
}
/// `linearToHex(rgb)`.
fn linear_to_hex(rgb: [f64; 3]) -> String {
    let mut s = String::from("#");
    for c in rgb {
        let v = round(1.0f64.min(0.0f64.max(lin_to_srgb(c))) * 255.0);
        let h = hex(v);
        if h.len() < 2 {
            s.push('0');
        }
        s.push_str(&h);
    }
    s
}

struct NodeOut {
    name: String,
    matrix: Option<M4>,
    mesh: Option<usize>,
    children: Vec<usize>,
}
struct Prim {
    pa: usize,
    na: usize,
    ia: usize,
    material: usize,
}
struct MeshOut {
    name: String,
    prims: Vec<Prim>,
}
struct MatOut {
    name: String,
    base: Option<[f64; 3]>,
}
struct AccOut {
    view: usize,
    ctype: u32,
    count: usize,
    ty: &'static str,
    minmax: Option<([f64; 3], [f64; 3])>,
}
struct ViewOut {
    offset: usize,
    length: usize,
    target: u32,
}

struct Glb<'m> {
    model: &'m Model,
    nodes: Vec<NodeOut>,
    meshes: Vec<MeshOut>,
    materials: Vec<MatOut>,
    accessors: Vec<AccOut>,
    views: Vec<ViewOut>,
    bin: Vec<u8>,
    mat_index: FxMap<String, usize>,
    mesh_of: FxMap<String, Option<usize>>,
}

impl Glb<'_> {
    fn push_view(&mut self, bytes: &[u8], target: u32) -> usize {
        let pad = (4usize.saturating_sub(self.bin.len() % 4)) % 4;
        self.bin.resize(self.bin.len().saturating_add(pad), 0);
        self.views.push(ViewOut { offset: self.bin.len(), length: bytes.len(), target });
        self.bin.extend_from_slice(bytes);
        self.views.len().saturating_sub(1)
    }
    fn material(&mut self, color: Option<&str>) -> usize {
        let k = color.unwrap_or("").to_string();
        if let Some(&i) = self.mat_index.get(&k) {
            return i;
        }
        let name = match color {
            None => "Default".to_string(),
            Some(c) if is_material_paint(c) => format!("Material {}", c.get(4..).unwrap_or("")),
            Some(c) => format!("Colour {c}"),
        };
        self.materials.push(MatOut { name, base: color.map(hex_to_linear) });
        let i = self.materials.len().saturating_sub(1);
        self.mat_index.insert(k, i);
        i
    }
    fn mesh_for(&mut self, def_id: &str, dflt: Option<&str>) -> Option<usize> {
        let key = format!("{def_id}|{}", dflt.unwrap_or(""));
        if let Some(m) = self.mesh_of.get(&key) {
            return *m;
        }
        let def = self.model.def(def_id)?;
        let tris = def_tris(def, dflt);
        if tris.is_empty() {
            self.mesh_of.insert(key, None);
            return None;
        }
        let mut by_color: Vec<(String, Vec<usize>)> = Vec::new();
        for (i, t) in tris.iter().enumerate() {
            let k = t.color.clone().unwrap_or_default();
            match by_color.iter_mut().find(|(c, _)| *c == k) {
                Some((_, l)) => l.push(i),
                None => by_color.push((k, vec![i])),
            }
        }
        let mut prims = Vec::new();
        for (k, list) in by_color {
            let n = list.len().saturating_mul(3);
            let mut pos: Vec<f32> = Vec::with_capacity(n.saturating_mul(3));
            let mut nor: Vec<f32> = Vec::with_capacity(n.saturating_mul(3));
            for &ti in &list {
                let Some(t) = tris.get(ti) else { continue };
                for k2 in 0..3 {
                    let p = t.p.get(k2).copied().unwrap_or([f64::NAN; 3]);
                    let q = match &t.ns {
                        Some(ns) => ns.get(k2).copied().unwrap_or(t.n),
                        None => t.n,
                    };
                    pos.extend(p.iter().map(|&x| f32_of(x)));
                    nor.extend(q.iter().map(|&x| f32_of(x)));
                }
            }
            let mut min = [f64::INFINITY; 3];
            let mut max = [f64::NEG_INFINITY; 3];
            for c in pos.chunks(3) {
                for (a, &v) in c.iter().enumerate() {
                    let v = f64::from(v);
                    if let (Some(mn), Some(mx)) = (min.get_mut(a), max.get_mut(a)) {
                        if v < *mn {
                            *mn = v;
                        }
                        if v > *mx {
                            *mx = v;
                        }
                    }
                }
            }
            let pb: Vec<u8> = pos.iter().flat_map(|v| v.to_le_bytes()).collect();
            let nb: Vec<u8> = nor.iter().flat_map(|v| v.to_le_bytes()).collect();
            let ib: Vec<u8> = (0..n).flat_map(|i| u32::try_from(i).unwrap_or(u32::MAX).to_le_bytes()).collect();
            let pv = self.push_view(&pb, 34962);
            let nv = self.push_view(&nb, 34962);
            let iv = self.push_view(&ib, 34963);
            self.accessors.push(AccOut { view: pv, ctype: 5126, count: n, ty: "VEC3", minmax: Some((min, max)) });
            let pa = self.accessors.len().saturating_sub(1);
            self.accessors.push(AccOut { view: nv, ctype: 5126, count: n, ty: "VEC3", minmax: None });
            let na = self.accessors.len().saturating_sub(1);
            self.accessors.push(AccOut { view: iv, ctype: 5125, count: n, ty: "SCALAR", minmax: None });
            let ia = self.accessors.len().saturating_sub(1);
            let material = self.material(if k.is_empty() { None } else { Some(k.as_str()) });
            prims.push(Prim { pa, na, ia, material });
        }
        self.meshes.push(MeshOut { name: def.name.clone(), prims });
        let mi = self.meshes.len().saturating_sub(1);
        self.mesh_of.insert(key, Some(mi));
        Some(mi)
    }
    fn node_for(&mut self, def_id: &str, m: Option<&M4>, name: String, depth: u32, dflt: Option<&str>) -> usize {
        let matrix = m.filter(|m| !is_ident_m(m)).map(|m| m.map(|v| if v.abs() < 1e-15 { 0.0 } else { v }));
        self.nodes.push(NodeOut { name, matrix, mesh: None, children: Vec::new() });
        let idx = self.nodes.len().saturating_sub(1);
        let mesh = self.mesh_for(def_id, dflt);
        if let Some(n) = self.nodes.get_mut(idx) {
            n.mesh = mesh;
        }
        if depth < 32 {
            let mut kids = Vec::new();
            let insts: Vec<(String, M4, Option<String>, Option<String>)> = self
                .model
                .def(def_id)
                .map(|d| d.insts.values().map(|i| (i.def.clone(), i.m, i.name.clone(), i.color.clone())).collect())
                .unwrap_or_default();
            for (sub_id, im, iname, icolor) in insts {
                if self.nodes.len() >= MAX_NODES {
                    break;
                }
                let Some(d) = self.model.def(&sub_id) else { continue };
                let nm = iname.filter(|s| !s.is_empty()).unwrap_or_else(|| d.name.clone());
                let c = icolor.filter(|s| !s.is_empty()).or_else(|| dflt.map(str::to_string));
                kids.push(self.node_for(&sub_id, Some(&im), nm, depth.saturating_add(1), c.as_deref()));
            }
            if let Some(n) = self.nodes.get_mut(idx) {
                n.children = kids;
            }
        }
        idx
    }
}

/// A number as `JSON.stringify` writes it (the even shortest digits at a tie: [`crate::num::js_num`]).
fn jnum(o: &mut Out, x: f64) {
    if x.is_finite() {
        crate::num::js_num(x, &mut o.s);
    } else {
        o.num(x);
    }
}
fn jnums(o: &mut Out, xs: &[f64]) {
    o.ch('[');
    for (i, x) in xs.iter().enumerate() {
        if i > 0 {
            o.ch(',');
        }
        jnum(o, *x);
    }
    o.ch(']');
}

fn idx_list(o: &mut Out, xs: &[usize]) {
    o.ch('[');
    for (i, x) in xs.iter().enumerate() {
        if i > 0 {
            o.ch(',');
        }
        o.num(*x as f64);
    }
    o.ch(']');
}

/// `exportGLB(model)` → the bytes.
pub fn export_glb(model: &Model) -> Vec<u8> {
    let mut g = Glb {
        model,
        nodes: Vec::new(),
        meshes: Vec::new(),
        materials: Vec::new(),
        accessors: Vec::new(),
        views: Vec::new(),
        bin: Vec::new(),
        mat_index: FxMap::default(),
        mesh_of: FxMap::default(),
    };
    g.nodes.push(NodeOut { name: "ModelCraft (Z up → Y up)".into(), matrix: Some(Z2Y), mesh: None, children: Vec::new() });
    let root_name =
        model.root().map(|r| if r.name.is_empty() { "Model".to_string() } else { r.name.clone() }).unwrap_or_else(|| "Model".into());
    let top = g.node_for("root", None, root_name, 0, None);
    if let Some(n) = g.nodes.first_mut() {
        n.children = vec![top];
    }
    let mut o = Out::new(false);
    o.raw("{\"asset\":{\"version\":\"2.0\",\"generator\":\"ModelCraft\"},\"scene\":0,\"scenes\":[{\"nodes\":[0]}],\"nodes\":[");
    for (i, n) in g.nodes.iter().enumerate() {
        if i > 0 {
            o.ch(',');
        }
        o.raw("{\"name\":");
        o.str(&n.name);
        if let Some(m) = &n.matrix {
            o.raw(",\"matrix\":");
            jnums(&mut o, m);
        }
        if let Some(me) = n.mesh {
            o.raw(",\"mesh\":");
            o.num(me as f64);
        }
        if !n.children.is_empty() {
            o.raw(",\"children\":");
            idx_list(&mut o, &n.children);
        }
        o.ch('}');
    }
    o.ch(']');
    let has_meshes = !g.meshes.is_empty();
    if has_meshes {
        o.raw(",\"meshes\":[");
        for (i, m) in g.meshes.iter().enumerate() {
            if i > 0 {
                o.ch(',');
            }
            o.raw("{\"name\":");
            o.str(&m.name);
            o.raw(",\"primitives\":[");
            for (j, p) in m.prims.iter().enumerate() {
                if j > 0 {
                    o.ch(',');
                }
                o.raw("{\"attributes\":{\"POSITION\":");
                o.num(p.pa as f64);
                o.raw(",\"NORMAL\":");
                o.num(p.na as f64);
                o.raw("},\"indices\":");
                o.num(p.ia as f64);
                o.raw(",\"material\":");
                o.num(p.material as f64);
                o.raw(",\"mode\":4}");
            }
            o.raw("]}");
        }
        o.ch(']');
    }
    if !g.materials.is_empty() {
        o.raw(",\"materials\":[");
        for (i, m) in g.materials.iter().enumerate() {
            if i > 0 {
                o.ch(',');
            }
            o.raw("{\"name\":");
            o.str(&m.name);
            o.raw(",\"pbrMetallicRoughness\":{\"metallicFactor\":0,\"roughnessFactor\":0.9");
            if let Some(b) = &m.base {
                o.raw(",\"baseColorFactor\":");
                jnums(&mut o, &[b[0], b[1], b[2], 1.0]);
            }
            o.raw("},\"doubleSided\":true}");
        }
        o.ch(']');
    }
    let mut byte_length = g.bin.len();
    if has_meshes {
        o.raw(",\"accessors\":[");
        for (i, a) in g.accessors.iter().enumerate() {
            if i > 0 {
                o.ch(',');
            }
            o.raw("{\"bufferView\":");
            o.num(a.view as f64);
            o.raw(",\"componentType\":");
            o.num(f64::from(a.ctype));
            o.raw(",\"count\":");
            o.num(a.count as f64);
            o.raw(",\"type\":");
            o.str(a.ty);
            if let Some((mn, mx)) = &a.minmax {
                o.raw(",\"min\":");
                jnums(&mut o, mn);
                o.raw(",\"max\":");
                jnums(&mut o, mx);
            }
            o.ch('}');
        }
        o.raw("],\"bufferViews\":[");
        for (i, v) in g.views.iter().enumerate() {
            if i > 0 {
                o.ch(',');
            }
            o.raw("{\"buffer\":0,\"byteOffset\":");
            o.num(v.offset as f64);
            o.raw(",\"byteLength\":");
            o.num(v.length as f64);
            o.raw(",\"target\":");
            o.num(f64::from(v.target));
            o.ch('}');
        }
        o.raw("],\"buffers\":[{\"byteLength\":");
        o.num(byte_length as f64);
        o.raw("}]");
    } else {
        byte_length = 0;
    }
    o.ch('}');
    let json = o.s.into_bytes();
    let jpad = (4usize.saturating_sub(json.len() % 4)) % 4;
    let jlen = json.len().saturating_add(jpad);
    let bin_pad = (4usize.saturating_sub(byte_length % 4)) % 4;
    let bin_len = byte_length.saturating_add(bin_pad);
    let total = 20usize.saturating_add(jlen).saturating_add(if byte_length > 0 { bin_len.saturating_add(8) } else { 0 });
    let u = |x: usize| u32::try_from(x).unwrap_or(u32::MAX).to_le_bytes();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&0x4654_6c67u32.to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&u(total));
    out.extend_from_slice(&u(jlen));
    out.extend_from_slice(&0x4e4f_534au32.to_le_bytes());
    out.extend_from_slice(&json);
    out.resize(out.len().saturating_add(jpad), 0x20);
    if byte_length > 0 {
        out.extend_from_slice(&u(bin_len));
        out.extend_from_slice(&0x004e_4942u32.to_le_bytes());
        out.extend_from_slice(&g.bin);
        out.resize(total, 0);
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------
// import

/// An accessor read in place: element i, component c is at `ao + i·stride + c·size` of `buf` (None = no buffer view:
/// zeros).
#[derive(Clone, Copy)]
struct Acc<'b> {
    buf: Option<&'b [u8]>,
    ao: usize,
    stride: usize,
    size: usize,
    ctype: u32,
    nc: usize,
    count: usize,
}

impl Acc<'_> {
    /// `data[k]` of the Float64Array the JS fills (k = i·nc + c), as a typed array answers: a non-integer or out of
    /// range k is undefined, which becomes NaN in arithmetic.
    fn data(&self, k: f64) -> f64 {
        let Some(ku) = crate::num::index(k) else { return f64::NAN };
        if ku >= self.count.saturating_mul(self.nc) {
            return f64::NAN;
        }
        let Some(buf) = self.buf else { return 0.0 };
        let i = ku.checked_div(self.nc).unwrap_or(0);
        let c = ku.checked_rem(self.nc).unwrap_or(0);
        let off = self.ao.saturating_add(i.saturating_mul(self.stride)).saturating_add(c.saturating_mul(self.size));
        let b = |n: usize| buf.get(off..off.saturating_add(n));
        let b2 = || b(2).and_then(|x| <[u8; 2]>::try_from(x).ok());
        let b4 = || b(4).and_then(|x| <[u8; 4]>::try_from(x).ok());
        match self.ctype {
            5120 => b(1).and_then(|x| x.first().copied()).map_or(f64::NAN, |x| f64::from(i8::from_le_bytes([x]))),
            5121 => b(1).and_then(|x| x.first().copied()).map_or(f64::NAN, f64::from),
            5122 => b2().map_or(f64::NAN, |x| f64::from(i16::from_le_bytes(x))),
            5123 => b2().map_or(f64::NAN, |x| f64::from(u16::from_le_bytes(x))),
            5125 => b4().map_or(f64::NAN, |x| f64::from(u32::from_le_bytes(x))),
            _ => b4().map_or(f64::NAN, |x| f64::from(f32::from_le_bytes(x))),
        }
    }
    /// The whole Float64Array.
    fn all(&self) -> Vec<f64> {
        (0..self.count.saturating_mul(self.nc)).map(|k| self.data(k as f64)).collect()
    }
}

/// `JSON.parse(new TextDecoder().decode(bytes))` (the decoder drops a leading byte-order mark). Spec gap: serde_json
/// refuses what V8 accepts in three corners — numbers beyond ±1.8e308 (V8: ±Infinity), lone surrogate escapes, and
/// nesting deeper than 128 — such a file reads as broken JSON here.
fn parse_json_bytes(b: &[u8]) -> Option<Value> {
    let b = b.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(b);
    let text = String::from_utf8_lossy(b);
    serde_json::from_str::<Value>(&text).ok()
}

/// `u8.subarray(begin, end)` with JS's relative-index rules.
fn subarray(len: usize, begin: f64, end: f64) -> (usize, usize) {
    let rel = |x: f64| -> usize {
        let x = if x.is_nan() { 0.0 } else { x.trunc() };
        let l = len as f64;
        let v = if x < 0.0 { (l + x).max(0.0) } else { x.min(l) };
        crate::num::index(v).unwrap_or(0)
    };
    let a = rel(begin);
    (a, rel(end).max(a))
}

/// `(x || 0) + n` in JS for a JSON value x: a number adds, text (and arrays) joins. → (begin as a number, the sum as a
/// number).
fn js_or0_add(x: Option<&Value>, n: f64) -> (f64, f64) {
    match x {
        Some(v) if truthy(Some(v)) => match v {
            Value::Number(k) => {
                let k = k.as_f64().unwrap_or(f64::NAN);
                (k, k + n)
            }
            Value::Bool(_) => (1.0, 1.0 + n),
            other => {
                let s = crate::jsx::to_js_string(Some(other));
                (crate::num::number(&s), crate::num::number(&format!("{s}{}", crate::num::s(n))))
            }
        },
        _ => (0.0, n),
    }
}

/// What a material's base colour texture became: the image id, its texture coordinate set, its colour from afar.
#[derive(Clone)]
struct Tex {
    id: String,
    tc: f64,
    avg: Option<String>,
}

/// The key a JS Map uses for a material reference (numbers and strings by value; arrays and objects are their own
/// identity, so never shared).
fn cache_key(v: Option<&Value>) -> Option<String> {
    match v {
        None => Some("u".into()),
        Some(Value::Null) => Some("null".into()),
        Some(Value::Bool(b)) => Some(format!("b{b}")),
        Some(Value::Number(n)) => Some(format!("n{}", crate::num::s(n.as_f64().unwrap_or(f64::NAN)))),
        Some(Value::String(s)) => Some(format!("s{s}")),
        _ => None,
    }
}

fn yup_to_zup(p: P3) -> P3 {
    [p[0], -p[2], p[1]]
}

fn is_objlike(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Object(_)) | Some(Value::Array(_)))
}

fn fail(m: &str) -> cc_m3d::Error {
    err_with("m3d.io.gltf_bad", &[("why", m)])
}
fn fail_t(k: &str) -> cc_m3d::Error {
    fail(&t(k, &[]))
}

/// Most triangles an import makes (a mesh placed by many nodes multiplies; the JS would run out of memory first).
const MAX_TRIS: usize = MAX_VERTS * 3;

struct Imp<'a> {
    views: &'a [Value],
    accs: &'a [Value],
    mats: &'a [Value],
    images: &'a [Value],
    textures: &'a [Value],
    meshes: &'a [Value],
    nodes: &'a [Value],
    buffers: Vec<Vec<u8>>,
    budget: i64,
    skipped: Map<String, Value>,
    n_meshes: f64,
    tex_cache: FxMap<String, Option<Tex>>,
    tris: Vec<([P3; 3], Option<String>)>,
    visits: u64,
}

impl<'a> Imp<'a> {
    /// `readAcc(ai)` (borrows the buffers; the budget is counted down).
    fn read_acc<'s>(buffers: &'s [Vec<u8>], views: &[Value], accs: &[Value], budget: &mut i64, ai: Option<&Value>) -> Res<Acc<'s>> {
        let a = at_key(accs, ai);
        if !is_objlike(a) {
            return Err(fail_t("m3d.io.an_accessor_is_missing"));
        }
        if truthy(get(a, "sparse")) {
            return Err(fail_t("m3d.io.sparse_accessors_are_not_supported"));
        }
        // Spec gap: COMP / NCOMP are JS objects, so a type named after an Object.prototype member ("constructor",
        // "toString", …) reads that member there; here it is unknown.
        let (size, ctype) = match crate::jsx::to_js_string(get(a, "componentType")).as_str() {
            "5120" => (1usize, 5120u32),
            "5121" => (1, 5121),
            "5122" => (2, 5122),
            "5123" => (2, 5123),
            "5125" => (4, 5125),
            "5126" => (4, 5126),
            _ => return Err(fail_t("m3d.io.an_accessor_has_an_unknown_number")),
        };
        let nc = match crate::jsx::to_js_string(get(a, "type")).as_str() {
            "SCALAR" => 1usize,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            "MAT4" => 16,
            _ => return Err(fail_t("m3d.io.an_accessor_has_an_unknown_shape")),
        };
        let count = match int(get(a, "count")) {
            Some(c) if c >= 0.0 && c <= (MAX_VERTS * 3) as f64 => crate::num::index(c).unwrap_or(0),
            _ => return Err(fail_t("m3d.io.an_accessor_is_too_big")),
        };
        *budget = budget.saturating_sub(i64::try_from(count).unwrap_or(i64::MAX));
        if *budget < 0 {
            return Err(err("m3d.io.this_gltf_file_is_too_big"));
        }
        let bv = get(a, "bufferView");
        if bv.is_none() {
            return Ok(Acc { buf: None, ao: 0, stride: 0, size, ctype, nc, count });
        }
        let v = at_key(views, bv);
        if !is_objlike(v) {
            return Err(fail_t("m3d.io.a_buffer_view_is_missing"));
        }
        let Some(buf) = crate::jsx::key_index(get(v, "buffer")).and_then(|i| buffers.get(i)) else {
            return Err(fail_t("m3d.io.a_buffer_view_points_at_a"));
        };
        let or0 = |x: Option<&Value>| -> Option<f64> { if truthy(x) { int(x) } else { Some(0.0) } };
        let (Some(vo), Some(vl)) = (or0(get(v, "byteOffset")), int(get(v, "byteLength"))) else {
            return Err(fail_t("m3d.io.a_buffer_view_runs_past_its"));
        };
        if vo < 0.0 || vo + vl > buf.len() as f64 {
            return Err(fail_t("m3d.io.a_buffer_view_runs_past_its"));
        }
        let snc = (size.saturating_mul(nc)) as f64;
        let stride_v = get(v, "byteStride");
        let stride = if truthy(stride_v) { num(stride_v) } else { Some(snc) };
        let Some(stride) = stride.filter(|s| is_integer(*s) && *s >= snc) else {
            return Err(fail_t("m3d.io.a_buffer_view_stride_is_wrong"));
        };
        let ao = or0(get(a, "byteOffset"));
        if count > 0 {
            match ao {
                Some(x) if x >= 0.0 && x + stride * (count as f64 - 1.0) + snc <= vl => {}
                _ => return Err(fail_t("m3d.io.an_accessor_runs_past_its_buffer")),
            }
        }
        // new DataView(buf.buffer, buf.byteOffset + vo, vl): a negative length is a RangeError
        if vl < 0.0 {
            return Err(cc_m3d::Error::Msg("$RangeError".into()));
        }
        let vo_u = crate::num::index(vo).unwrap_or(0);
        let vl_u = crate::num::index(vl).unwrap_or(0);
        Ok(Acc {
            buf: Some(buf.get(vo_u..vo_u.saturating_add(vl_u)).unwrap_or(&[])),
            ao: ao.and_then(crate::num::index).unwrap_or(0),
            stride: crate::num::index(stride).unwrap_or(0),
            size,
            ctype,
            nc,
            count,
        })
    }

    /// `colorOf(mi)`: the material's base colour factor as sRGB hex, or None.
    fn color_of(&self, mi: Option<&Value>) -> Option<String> {
        let f = get(get(at_key(self.mats, mi), "pbrMetallicRoughness"), "baseColorFactor");
        let a = f.and_then(Value::as_array)?;
        if a.len() < 3 {
            return None;
        }
        let mut rgb = [0.0; 3];
        for (k, slot) in rgb.iter_mut().enumerate() {
            *slot = crate::jsx::finite(a.get(k))?;
        }
        Some(linear_to_hex(rgb))
    }

    /// `texOf(mi)`: base colour textures (0.5 RC.11 (b)) — an embedded PNG / JPEG / WebP kept in model.images.
    fn tex_of(&mut self, model: &mut Model, mi: Option<&Value>) -> Option<Tex> {
        let ck = cache_key(mi);
        if let Some(v) = ck.as_ref().and_then(|k| self.tex_cache.get(k)) {
            return v.clone();
        }
        let mut out: Option<Tex> = None;
        let mat = at_key(self.mats, mi);
        let bct = get(get(mat, "pbrMetallicRoughness"), "baseColorTexture");
        let img = if truthy(bct) && int(get(bct, "index")).is_some() {
            let tx = crate::jsx::at(self.textures, int(get(bct, "index")).unwrap_or(f64::NAN));
            at_key(self.images, get(tx, "source"))
        } else {
            None
        };
        if truthy(img) && is_objlike(img) {
            let mut bytes: Option<Vec<u8>> = None;
            if int(get(img, "bufferView")).is_some() {
                let v = crate::jsx::at(self.views, int(get(img, "bufferView")).unwrap_or(f64::NAN));
                let buf = if truthy(v) { crate::jsx::key_index(get(v, "buffer")).and_then(|i| self.buffers.get(i)) } else { None };
                if let Some(buf) = buf
                    && let Some(vl) = int(get(v, "byteLength"))
                {
                    let (beg, end) = js_or0_add(get(v, "byteOffset"), vl);
                    if end <= buf.len() as f64 {
                        let (a, b) = subarray(buf.len(), beg, end);
                        bytes = Some(buf.get(a..b).unwrap_or(&[]).to_vec());
                    }
                }
            } else if let Some(u) = get(img, "uri").and_then(Value::as_str) {
                let m = u.strip_prefix("data:image/").and_then(|r| {
                    let k = r.bytes().take_while(u8::is_ascii_lowercase).count();
                    if k == 0 {
                        return None;
                    }
                    r.get(k..)?.strip_prefix(";base64,")
                });
                if let Some(d) = m {
                    bytes = Some(base64_node(d));
                }
            }
            let mime = bytes.as_ref().filter(|b| b.len() > 12).and_then(|b| {
                let at = |i: usize| b.get(i).copied().unwrap_or(0);
                if at(0) == 0x89 && at(1) == 0x50 {
                    Some("image/png")
                } else if at(0) == 0xff && at(1) == 0xd8 {
                    Some("image/jpeg")
                } else if at(8) == 0x57 && at(9) == 0x45 && at(10) == 0x42 {
                    Some("image/webp")
                } else {
                    None
                }
            });
            match (mime, &bytes) {
                (Some(mime), Some(b)) => {
                    let nm_v = if truthy(get(img, "name")) {
                        get(img, "name")
                    } else if truthy(get(mat, "name")) {
                        get(mat, "name")
                    } else {
                        None
                    };
                    let nm = nm_v.map_or_else(|| "texture".to_string(), |v| crate::jsx::to_js_string(Some(v)));
                    let avg = self.color_of(mi);
                    let tc = int(get(bct, "texCoord")).unwrap_or(0.0);
                    out = model.add_image(&crate::util::slice16(&nm, 120), mime, &to_base64(b), avg.as_deref()).ok().map(|id| Tex {
                        id,
                        tc,
                        avg: self.color_of(mi),
                    });
                }
                _ => skip(&mut self.skipped, &t("m3d.io.textures_faces_keep_no_colour", &[]), 1.0),
            }
        }
        if let Some(k) = ck {
            self.tex_cache.insert(k, out.clone());
        }
        out
    }

    /// `meshAt(mi, m)`: one mesh placed by m.
    fn mesh_at(&mut self, model: &mut Model, mi: Option<&Value>, m: &M4) -> Res<()> {
        let mesh = at_key(self.meshes, mi);
        if !truthy(mesh) {
            return Ok(());
        }
        self.n_meshes += 1.0;
        let mirror = det3(m) < 0.0;
        for pr in arr(get(mesh, "primitives")) {
            if pr.is_null() {
                // `pr.mode` of null
                return Err(cc_m3d::Error::Type);
            }
            let pr = Some(pr);
            // pr.mode ?? 4, compared as JS compares (a string mode reads as its number)
            let mode_v = get(pr, "mode").filter(|v| !v.is_null());
            let mode_n = match mode_v {
                None => 4.0,
                Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
                Some(Value::Bool(b)) => f64::from(u8::from(*b)),
                Some(v) => crate::num::number(&crate::jsx::to_js_string(Some(v))),
            };
            let mode_is = |k: f64| match mode_v {
                None => k == 4.0,
                Some(Value::Number(n)) => n.as_f64() == Some(k),
                _ => false,
            };
            if mode_n < 4.0 || mode_n > 6.0 {
                skip(&mut self.skipped, &t(if mode_is(0.0) { "m3d.io.points" } else { "m3d.io.lines" }, &[]), 1.0);
                continue;
            }
            let attrs = get(pr, "attributes");
            let pos_ref = get(attrs, "POSITION");
            if pos_ref.is_none() {
                skip(&mut self.skipped, &t("m3d.io.primitive_without_positions", &[]), 1.0);
                continue;
            }
            let p = Self::read_acc(&self.buffers, self.views, self.accs, &mut self.budget, pos_ref)?;
            if p.nc != 3 {
                return Err(fail_t("m3d.io.positions_are_not_3d"));
            }
            let idx: Vec<f64> = match get(pr, "indices") {
                Some(r) => {
                    let ia = Self::read_acc(&self.buffers, self.views, self.accs, &mut self.budget, Some(r))?;
                    let v = ia.all();
                    if v.iter().any(|&i| i >= p.count as f64) {
                        return Err(fail_t("m3d.io.an_index_points_past_the_vertices"));
                    }
                    v
                }
                None => (0..p.count).map(|i| i as f64).collect(),
            };
            let g = |i: usize| idx.get(i).copied().unwrap_or(f64::NAN);
            let mut list: Vec<[f64; 3]> = Vec::new();
            if mode_is(4.0) {
                let mut i = 0usize;
                while i.saturating_add(2) < idx.len() {
                    list.push([g(i), g(i.saturating_add(1)), g(i.saturating_add(2))]);
                    i = i.saturating_add(3);
                }
            } else if mode_is(5.0) {
                let mut i = 0usize;
                while i.saturating_add(2) < idx.len() {
                    list.push(if i % 2 == 1 {
                        [g(i.saturating_add(1)), g(i), g(i.saturating_add(2))]
                    } else {
                        [g(i), g(i.saturating_add(1)), g(i.saturating_add(2))]
                    });
                    i = i.saturating_add(1);
                }
            } else {
                let mut i = 1usize;
                while i.saturating_add(1) < idx.len() {
                    list.push([g(0), g(i), g(i.saturating_add(1))]);
                    i = i.saturating_add(1);
                }
            }
            let mref = get(pr, "material");
            let color = if mref.is_some() { self.color_of(mref) } else { None };
            let tex = if mref.is_some() { self.tex_of(model, mref) } else { None };
            // the positions again (tex_of may have borrowed the buffers mutably through self)
            let p = Self::read_acc_again(&self.buffers, self.views, self.accs, pos_ref)?;
            let uv = match &tex {
                Some(tx) => match get(attrs, &format!("TEXCOORD_{}", crate::num::s(tx.tc))) {
                    Some(r) => Some(Self::read_acc(&self.buffers, self.views, self.accs, &mut self.budget, Some(r))?),
                    None => None,
                },
                None => None,
            };
            let pt = |i: f64| yup_to_zup(apply(m, [p.data(i * 3.0), p.data(i * 3.0 + 1.0), p.data(i * 3.0 + 2.0)]));
            for tr in &list {
                let ps = [pt(tr[0]), pt(tr[1]), pt(tr[2])];
                let mut c = color.clone();
                if let (Some(u), Some(tx)) = (&uv, &tex)
                    && u.nc == 2
                    && tr.iter().all(|&i| i < u.count as f64)
                {
                    let uvs: Vec<[f64; 2]> = tr.iter().map(|&i| [u.data(i * 2.0), 1.0 - u.data(i * 2.0 + 1.0)]).collect();
                    let n = cross(sub(ps[1], ps[0]), sub(ps[2], ps[0]));
                    c = image_paint_fit(&tx.id, &ps, &uvs, n, tx.avg.as_deref()).or_else(|| color.clone());
                }
                self.tris.push((if mirror { [ps[0], ps[2], ps[1]] } else { ps }, c));
                if self.tris.len() > MAX_TRIS {
                    return Err(err("m3d.io.this_gltf_file_is_too_big"));
                }
            }
        }
        Ok(())
    }

    /// The same accessor once more without counting it against the budget (the JS reads it once and keeps it).
    fn read_acc_again<'s>(buffers: &'s [Vec<u8>], views: &[Value], accs: &[Value], ai: Option<&Value>) -> Res<Acc<'s>> {
        let mut b = i64::MAX;
        Self::read_acc(buffers, views, accs, &mut b, ai)
    }

    /// `nodeMatrix(n)`.
    fn node_matrix(n: Option<&Value>) -> Res<M4> {
        if let Some(a) = get(n, "matrix").and_then(Value::as_array)
            && a.len() == 16
            && a.iter().all(|x| crate::jsx::finite(Some(x)).is_some())
        {
            let mut m = [0.0; 16];
            for (s, x) in m.iter_mut().zip(a) {
                *s = x.as_f64().unwrap_or(f64::NAN);
            }
            return Ok(m);
        }
        let list = |k: &str, d: &[f64]| -> Option<Vec<f64>> {
            match get(n, k).and_then(Value::as_array) {
                Some(a) => a.iter().map(|x| crate::jsx::finite(Some(x))).collect(),
                None => Some(d.to_vec()),
            }
        };
        let (Some(tr), Some(r), Some(s)) =
            (list("translation", &[0.0, 0.0, 0.0]), list("rotation", &[0.0, 0.0, 0.0, 1.0]), list("scale", &[1.0, 1.0, 1.0]))
        else {
            return Err(fail_t("m3d.io.a_node_transform_is_not_a"));
        };
        let g = |v: &[f64], i: usize| v.get(i).copied().unwrap_or(f64::NAN);
        let (x, y, z, w) = (g(&r, 0), g(&r, 1), g(&r, 2), g(&r, 3));
        let rm: M4 = [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y + z * w),
            2.0 * (x * z - y * w),
            0.0,
            2.0 * (x * y - z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z + x * w),
            0.0,
            2.0 * (x * z + y * w),
            2.0 * (y * z - x * w),
            1.0 - 2.0 * (x * x + y * y),
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        ];
        let sm: M4 = [g(&s, 0), 0.0, 0.0, 0.0, 0.0, g(&s, 1), 0.0, 0.0, 0.0, 0.0, g(&s, 2), 0.0, 0.0, 0.0, 0.0, 1.0];
        let tm: M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, g(&tr, 0), g(&tr, 1), g(&tr, 2), 1.0];
        Ok(mmul(&tm, &mmul(&rm, &sm)))
    }

    /// `visit(ni, m, 0)`: the node tree depth first (the JS recursion as an explicit stack), 64 deep at most, 200 000
    /// visits in all.
    fn visit(&mut self, model: &mut Model, start: Option<&'a Value>, m0: &M4) -> Res<()> {
        struct Frame<'v> {
            wm: M4,
            depth: u32,
            kids: &'v [Value],
            next: usize,
        }
        let mut stack: Vec<Frame<'a>> = Vec::new();
        let mut pending: Option<(Option<&'a Value>, M4, u32)> = Some((start, *m0, 0));
        loop {
            if let Some((n, pwm, depth)) = pending.take() {
                let ok = truthy(n) && is_objlike(n) && depth <= 64 && {
                    self.visits = self.visits.saturating_add(1);
                    self.visits <= 200_000
                };
                if ok {
                    let wm = mmul(&pwm, &Self::node_matrix(n)?);
                    if let Some(mr) = get(n, "mesh") {
                        self.mesh_at(model, Some(mr), &wm)?;
                    }
                    stack.push(Frame { wm, depth, kids: arr(get(n, "children")), next: 0 });
                }
            }
            let Some(top) = stack.last_mut() else { break };
            let Some(c) = top.kids.get(top.next) else {
                stack.pop();
                continue;
            };
            top.next = top.next.saturating_add(1);
            if let Some(ci) = int(Some(c)) {
                pending = Some((crate::jsx::at(self.nodes, ci), top.wm, top.depth.saturating_add(1)));
            }
        }
        Ok(())
    }
}

/// `importGLTF(model, bytes, name)` → {def, report}.
pub fn import_gltf(model: &mut Model, u8s: &[u8], name: &str) -> Res<Imported> {
    let mut json: Option<Value> = None;
    let mut bin: Option<Vec<u8>> = None;
    let rd = |o: usize| -> u32 { u8s.get(o..o.saturating_add(4)).and_then(|b| <[u8; 4]>::try_from(b).ok()).map_or(0, u32::from_le_bytes) };
    if u8s.len() >= 12 && rd(0) == 0x4654_6c67 {
        if rd(4) != 2 {
            return Err(fail_t("m3d.io.only_gltf_2_0_is_supported"));
        }
        let total = usize::try_from(rd(8)).unwrap_or(usize::MAX).min(u8s.len());
        let mut o = 12usize;
        while o.saturating_add(8) <= total {
            let clen = usize::try_from(rd(o)).unwrap_or(usize::MAX);
            let typ = rd(o.saturating_add(4));
            let end = o.saturating_add(8).saturating_add(clen);
            if end > total {
                return Err(fail_t("m3d.io.a_chunk_runs_past_the_end"));
            }
            let data = u8s.get(o.saturating_add(8)..end).unwrap_or(&[]);
            if typ == 0x4e4f_534a && !truthy(json.as_ref()) {
                match parse_json_bytes(data) {
                    Some(v) => json = Some(v),
                    None => return Err(fail_t("m3d.io.its_json_part_is_broken")),
                }
            } else if typ == 0x004e_4942 && bin.is_none() {
                bin = Some(data.to_vec());
            }
            o = end;
        }
        if !truthy(json.as_ref()) {
            return Err(fail_t("m3d.io.it_has_no_json_part"));
        }
    } else {
        match parse_json_bytes(u8s) {
            Some(v) => json = Some(v),
            None => return Err(fail_t("m3d.io.it_is_neither_a_glb_nor")),
        }
    }
    let json = json.unwrap_or(Value::Null);
    let j = Some(&json);
    let asset = get(j, "asset");
    let ver = if matches!(asset, Some(Value::Object(_))) { get(asset, "version") } else { None };
    let ver_s = if truthy(ver) { crate::jsx::to_js_string(ver) } else { String::new() };
    if !truthy(j) || !is_objlike(j) || !truthy(asset) || ver_s.split('.').next() != Some("2") {
        return Err(fail_t("m3d.io.only_gltf_2_0_is_supported"));
    }
    let mut buffers: Vec<Vec<u8>> = Vec::new();
    for (i, b) in arr(get(j, "buffers")).iter().enumerate() {
        if !is_objlike(Some(b)) {
            return Err(fail_t("m3d.io.a_buffer_is_wrong"));
        }
        match get(Some(b), "uri") {
            None => match (&bin, i) {
                (Some(bb), 0) => buffers.push(bb.clone()),
                _ => return Err(fail_t("m3d.io.a_buffer_has_no_data")),
            },
            Some(u) => {
                let m = u.as_str().and_then(|s| {
                    let rest = s.strip_prefix("data:")?;
                    let semi = rest.find([';', ','])?;
                    rest.get(semi..)?.strip_prefix(";base64,")
                });
                match m {
                    Some(d) => buffers.push(base64_node(d)),
                    None => {
                        let us = crate::jsx::to_js_string(Some(u));
                        return Err(err_with("m3d.io.gltf_external", &[("uri", &crate::util::slice16(&us, 60))]));
                    }
                }
            }
        }
    }
    let mut imp = Imp {
        views: arr(get(j, "bufferViews")),
        accs: arr(get(j, "accessors")),
        mats: arr(get(j, "materials")),
        images: arr(get(j, "images")),
        textures: arr(get(j, "textures")),
        meshes: arr(get(j, "meshes")),
        nodes: arr(get(j, "nodes")),
        buffers,
        budget: i64::try_from(MAX_VERTS * 3).unwrap_or(i64::MAX),
        skipped: Map::new(),
        n_meshes: 0.0,
        tex_cache: FxMap::default(),
        tris: Vec::new(),
        visits: 0,
    };
    let scenes = arr(get(j, "scenes"));
    let scene = crate::jsx::at(scenes, int(get(j, "scene")).unwrap_or(0.0));
    if truthy(scene) {
        for ni in arr(get(scene, "nodes")) {
            let n = at_key(imp.nodes, Some(ni));
            imp.visit(model, n, &IDENT)?;
        }
    } else if !imp.nodes.is_empty() {
        // a node no other node lists as a child is a root (Set.has: numbers by value, -0 = 0)
        let mut child: Vec<f64> = Vec::new();
        for n in imp.nodes {
            for c in arr(get(Some(n), "children")) {
                if let Some(x) = num(Some(c)) {
                    child.push(x + 0.0);
                }
            }
        }
        child.sort_by(f64::total_cmp);
        let nodes = imp.nodes;
        for (i, n) in nodes.iter().enumerate() {
            let fi = i as f64;
            if child.binary_search_by(|x| x.total_cmp(&fi)).is_err() {
                imp.visit(model, Some(n), &IDENT)?;
            }
        }
    } else {
        for i in 0..imp.meshes.len() {
            imp.mesh_at(model, Some(&Value::from(i as f64)), &IDENT)?;
        }
    }
    let n_tris = imp.tris.len();
    let polys: Vec<PolyIn> = imp.tris.iter().map(|(p, _)| PolyIn::outline(p.to_vec())).collect();
    let id = mesh_def_in(model, &polys, Some(&clean_name(name)), "group", &[]);
    drop(polys);
    // colours: meshDef keeps no per-polygon data, so match faces back to triangles by centroid
    if imp.tris.iter().any(|(_, c)| c.as_deref().is_some_and(|s| !s.is_empty())) {
        let k = |p: P3| -> String {
            let mut s = String::new();
            for (i, x) in p.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                cc_m3d::js::push_num(&mut s, round(x * 1e4));
            }
            s
        };
        let mut by_c: FxMap<String, String> = FxMap::default();
        for (p, c) in &imp.tris {
            if let Some(c) = c.as_deref().filter(|s| !s.is_empty()) {
                by_c.insert(k(mul(add(add(p[0], p[1]), p[2]), 1.0 / 3.0)), c.to_string());
            }
        }
        if let Some(d) = model.def_mut(&id) {
            let verts = d.verts.clone();
            for f in d.faces.values_mut() {
                let l0 = f.loops.first().map_or(&[][..], Vec::as_slice);
                let mut s = [0.0; 3];
                for &v in l0 {
                    s = add(s, verts.get(v).copied().unwrap_or([f64::NAN; 3]));
                }
                if let Some(c) = by_c.get(&k(mul(s, 1.0 / l0.len() as f64))) {
                    f.color = Some(c.clone());
                }
            }
        }
    }
    let faces = model.def(&id).map_or(0, |d| d.faces.len());
    let mut report = Map::new();
    report.insert("triangles".into(), Value::from(n_tris as f64));
    report.insert("faces".into(), Value::from(faces as f64));
    report.insert("meshes".into(), Value::from(imp.n_meshes));
    report.insert("skipped".into(), Value::Object(imp.skipped));
    Ok(Imported { def: id, report, units: None })
}
