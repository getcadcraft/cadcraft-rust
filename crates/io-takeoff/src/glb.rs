//! `glb.js` — the 3D check view as a binary glTF 2.0 file: one node and one mesh per object (flat-normal triangles, or
//! lines for flat runs), one material per item colour (slabs and plates see-through); feet Z up → metres Y up.
//! Byte-identical to the JS for the same scene.
// Integer arithmetic here is buffer offsets and vertex indices of the scene being written (its lists are built from
// capped project data and live in memory): it cannot overflow, so the crate-wide deny is relaxed for this module.
#![allow(clippy::arithmetic_side_effects)]

use std::borrow::Cow;

use serde_json::{Map, Value, json};

use cc_takeoff::scene3d::Scene;

use cc_takeoff::jv::{self, n};

const M: f64 = 0.3048;

/// '#rrggbb' → linear [r, g, b].
pub fn linear_rgb(hex: &str) -> [f64; 3] {
    let ok = hex.len() == 7 && hex.starts_with('#') && hex.bytes().skip(1).all(|c| c.is_ascii_hexdigit());
    let v = if ok { u32::from_str_radix(hex.get(1..).unwrap_or("888888"), 16).unwrap_or(0x88_8888) } else { 0x88_8888 };
    [(v >> 16) & 255, (v >> 8) & 255, v & 255].map(|c| {
        let s = f64::from(c) / 255.0;
        if s <= 0.04045 { s / 12.92 } else { cc_takeoff::jsmath::pow((s + 0.055) / 1.055, 2.4) }
    })
}

fn to_gl(a: &[f64], i: usize) -> [f64; 3] {
    let g = |k: usize| a.get(i + k).copied().unwrap_or(f64::NAN);
    [g(0) * M, g(2) * M, -g(1) * M]
}

fn f32of(v: f64) -> f32 {
    #[allow(clippy::cast_possible_truncation)]
    let f = v as f32;
    f
}

/// One object of a scene as the writer reads it (from the scene JSON, or borrowed from a typed [`Scene`]).
struct GObj<'a> {
    kind: Cow<'a, str>,
    tris: Cow<'a, [f64]>,
    lines: Cow<'a, [f64]>,
    color: String,
    name: Option<String>,
    id: Cow<'a, Value>,
    item: Cow<'a, Value>,
    measure: Option<Cow<'a, Value>>,
}

/// A scene (`buildScene` / `buildStack` JSON) → GLB bytes.
pub fn scene_to_glb(scene: &Value, name: &str) -> Vec<u8> {
    let objs = jv::arr(scene.get("objects")).map_or(&[][..], Vec::as_slice).iter().map(|o| {
        let nums =
            |k: &str| -> Vec<f64> { jv::arr(o.get(k)).map(|l| l.iter().map(|x| jv::to_number(Some(x))).collect()).unwrap_or_default() };
        GObj {
            kind: Cow::Owned(jv::to_string(o.get("kind"))),
            tris: Cow::Owned(nums("tris")),
            lines: Cow::Owned(nums("lines")),
            color: jv::to_string_or_empty(o.get("color")),
            name: o.get("name").filter(|v| jv::truthy(Some(v))).map(|v| jv::to_string(Some(v))),
            id: Cow::Owned(o.get("id").cloned().unwrap_or(Value::Null)),
            item: Cow::Owned(o.get("item").cloned().unwrap_or(Value::Null)),
            measure: o.get("measure").map(|m| Cow::Owned(m.clone())),
        }
    });
    glb_of(objs, name)
}

/// [`scene_to_glb`] of a typed scene, without its JSON: the same bytes as `scene_to_glb(&scene.to_json(), name)`
/// (S.3: the JSON round trip of every coordinate was most of the writer's time).
pub fn scene_glb(scene: &Scene, name: &str) -> Vec<u8> {
    // what the JSON carries: a non-finite coordinate is written as null, which reads back as 0
    fn fin(l: &[f64]) -> Cow<'_, [f64]> {
        if l.iter().all(|v| v.is_finite()) {
            Cow::Borrowed(l)
        } else {
            Cow::Owned(l.iter().map(|v| if v.is_finite() { *v } else { 0.0 }).collect())
        }
    }
    let objs = scene.objects.iter().map(|o| GObj {
        kind: Cow::Borrowed(o.kind),
        tris: fin(&o.tris),
        lines: fin(&o.lines),
        color: jv::to_string_or_empty(Some(&o.color)),
        name: Some(&o.name).filter(|v| jv::truthy(Some(v))).map(|v| jv::to_string(Some(v))),
        id: Cow::Borrowed(&o.id),
        item: Cow::Borrowed(&o.item),
        measure: Some(Cow::Borrowed(&o.measure)),
    });
    glb_of(objs, name)
}

fn glb_of<'a>(objs: impl Iterator<Item = GObj<'a>>, name: &str) -> Vec<u8> {
    let mut chunks: Vec<Vec<f32>> = Vec::new();
    let mut byte_length = 0usize;
    let (mut buffer_views, mut accessors, mut meshes, mut nodes, mut materials) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::<Value>::new());
    let mut mat_index: Vec<(String, usize)> = Vec::new();
    let mut material = |materials: &mut Vec<Value>, color: &str, see: bool| -> usize {
        let key = format!("{color}{}", if see { "/see" } else { "" });
        if let Some((_, i)) = mat_index.iter().find(|(k, _)| *k == key) {
            return *i;
        }
        let [r, g, b] = linear_rgb(color);
        materials.push(json!({
            "name": key,
            "pbrMetallicRoughness": { "baseColorFactor": [n(r), n(g), n(b), n(if see { 0.78 } else { 1.0 })], "metallicFactor": 0, "roughnessFactor": 0.9 },
            "alphaMode": if see { "BLEND" } else { "OPAQUE" },
            "doubleSided": true,
        }));
        mat_index.push((key, materials.len() - 1));
        materials.len() - 1
    };
    let mut add_accessor = |buffer_views: &mut Vec<Value>,
                            accessors: &mut Vec<Value>,
                            chunks: &mut Vec<Vec<f32>>,
                            arr: Vec<f32>,
                            ty: &str,
                            with_bounds: bool|
     -> usize {
        let bl = arr.len() * 4;
        buffer_views.push(json!({ "buffer": 0, "byteOffset": byte_length, "byteLength": bl, "target": 34962 }));
        byte_length += bl;
        let mut acc = Map::new();
        acc.insert("bufferView".into(), json!(buffer_views.len() - 1));
        acc.insert("componentType".into(), json!(5126));
        acc.insert("count".into(), n(arr.len() as f64 / 3.0));
        acc.insert("type".into(), Value::String(ty.into()));
        if with_bounds {
            let mut mn = [f64::INFINITY; 3];
            let mut mx = [f64::NEG_INFINITY; 3];
            for c in arr.chunks(3) {
                for (k, v) in c.iter().enumerate() {
                    if let (Some(lo), Some(hi)) = (mn.get_mut(k), mx.get_mut(k)) {
                        *lo = cc_m3d::js::min(*lo, f64::from(*v));
                        *hi = cc_m3d::js::max(*hi, f64::from(*v));
                    }
                }
            }
            acc.insert("min".into(), json!([n(mn[0]), n(mn[1]), n(mn[2])]));
            acc.insert("max".into(), json!([n(mx[0]), n(mx[1]), n(mx[2])]));
        }
        accessors.push(Value::Object(acc));
        chunks.push(arr);
        accessors.len() - 1
    };
    for o in objs {
        let kind = o.kind.as_ref();
        if kind == "frame" {
            continue;
        }
        let (tris, lines, color) = (o.tris.as_ref(), o.lines.as_ref(), o.color.as_str());
        let mut prims = Vec::new();
        if tris.len() >= 9 {
            let nv = tris.len() / 3;
            let mut pos = vec![0f32; nv * 3];
            let mut nor = vec![0f32; nv * 3];
            let mut v = 0usize;
            while v < nv {
                let (a, b, c) = (to_gl(tris, v * 3), to_gl(tris, v * 3 + 3), to_gl(tris, v * 3 + 6));
                let (ux, uy, uz) = (b[0] - a[0], b[1] - a[1], b[2] - a[2]);
                let (vx, vy, vz) = (c[0] - a[0], c[1] - a[1], c[2] - a[2]);
                let (mut nx, mut ny, mut nz) = (uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx);
                let l = cc_m3d::js::hypot3(nx, ny, nz);
                if l > 0.0 {
                    nx /= l;
                    ny /= l;
                    nz /= l;
                } else {
                    nx = 0.0;
                    ny = 1.0;
                    nz = 0.0;
                }
                for (k, p) in [a, b, c].iter().enumerate() {
                    for (d, (pv, nv)) in p.iter().zip([nx, ny, nz]).enumerate() {
                        if let Some(slot) = pos.get_mut((v + k) * 3 + d) {
                            *slot = f32of(*pv);
                        }
                        if let Some(slot) = nor.get_mut((v + k) * 3 + d) {
                            *slot = f32of(nv);
                        }
                    }
                }
                v += 3;
            }
            let pa = add_accessor(&mut buffer_views, &mut accessors, &mut chunks, pos, "VEC3", true);
            let na = add_accessor(&mut buffer_views, &mut accessors, &mut chunks, nor, "VEC3", false);
            let mi = material(&mut materials, color, kind == "slab" || kind == "plate");
            prims.push(json!({ "attributes": { "POSITION": pa, "NORMAL": na }, "material": mi, "mode": 4 }));
        } else if kind == "run" && lines.len() >= 6 {
            let nv = lines.len() / 3;
            let mut pos = vec![0f32; nv * 3];
            for v in 0..nv {
                let p = to_gl(lines, v * 3);
                for (d, pv) in p.iter().enumerate() {
                    if let Some(slot) = pos.get_mut(v * 3 + d) {
                        *slot = f32of(*pv);
                    }
                }
            }
            let pa = add_accessor(&mut buffer_views, &mut accessors, &mut chunks, pos, "VEC3", true);
            let mi = material(&mut materials, color, false);
            prims.push(json!({ "attributes": { "POSITION": pa }, "material": mi, "mode": 1 }));
        }
        if prims.is_empty() {
            continue;
        }
        let oname = o.name;
        meshes.push(json!({ "name": oname.clone().unwrap_or_else(|| kind.to_string()), "primitives": prims }));
        let mut extras = Map::new();
        extras.insert("mark".into(), o.id.into_owned());
        extras.insert("item".into(), o.item.into_owned());
        extras.insert("kind".into(), Value::String(kind.to_string()));
        if let Some(m) = o.measure {
            extras.insert("measure".into(), m.into_owned());
        }
        nodes.push(json!({ "name": format!("{} ({kind})", oname.unwrap_or_default()), "mesh": meshes.len() - 1, "extras": extras }));
    }
    let mut gltf = Map::new();
    gltf.insert("asset".into(), json!({ "version": "2.0", "generator": "TakeoffCraft (glb.js)" }));
    gltf.insert("scene".into(), json!(0));
    gltf.insert("scenes".into(), json!([{ "name": name, "nodes": (0..nodes.len()).collect::<Vec<_>>() }]));
    let has = !nodes.is_empty();
    gltf.insert("nodes".into(), Value::Array(nodes));
    if has {
        gltf.insert("meshes".into(), Value::Array(meshes));
        gltf.insert("materials".into(), Value::Array(materials));
        gltf.insert("accessors".into(), Value::Array(accessors));
        gltf.insert("bufferViews".into(), Value::Array(buffer_views));
        gltf.insert("buffers".into(), json!([{ "byteLength": byte_length }]));
    }
    let json = jv::stringify(&Value::Object(gltf)).into_bytes();
    let pad = (4 - json.len() % 4) % 4;
    let json_len = json.len() + pad;
    let bin_len = if has { byte_length } else { 0 };
    let total = 12 + 8 + json_len + if bin_len > 0 { 8 + bin_len } else { 0 };
    let mut out = Vec::with_capacity(total);
    let w32 = |out: &mut Vec<u8>, v: usize| out.extend_from_slice(&u32::try_from(v).unwrap_or(u32::MAX).to_le_bytes());
    w32(&mut out, 0x4654_6C67);
    w32(&mut out, 2);
    w32(&mut out, total);
    w32(&mut out, json_len);
    w32(&mut out, 0x4E4F_534A);
    out.extend_from_slice(&json);
    out.extend(std::iter::repeat_n(0x20u8, pad));
    if bin_len > 0 {
        w32(&mut out, bin_len);
        w32(&mut out, 0x004E_4942);
        for c in &chunks {
            for v in c {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
    }
    out
}
