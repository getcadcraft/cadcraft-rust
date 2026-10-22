//! A ModelCraft model (cc-m3d) as a 3D view scene, built the way ModelCraft's view3d.js builds its three.js scene: one
//! geometry per definition and placement colour (a painted group fills its faces that have none), faces in their paint
//! or the front colour, smooth normals across soft edges, hidden faces and soft or hidden edges left out, every
//! placement through `walk_shown` (hidden placements and hidden tags skipped).

use std::collections::HashMap;

use cc_m3d::def::{Def, face_normal, face_tris};
use cc_m3d::model::Model;
use cc_render::mesh3d::{IDENT, Instance3, MeshData};

use crate::view3d::Scene3D;

/// The front colour of an unpainted face (view3d.js: paper '#f2efe7', model '#c9c3b5').
pub const FRONT_PAPER: [u8; 4] = [0xf2, 0xef, 0xe7, 255];
pub const FRONT_MODEL: [u8; 4] = [0xc9, 0xc3, 0xb5, 255];
/// The back colour (view3d.js: paper '#a9bacb', model '#5d7186').
pub const BACK_PAPER: [f32; 3] = [0xa9 as f32 / 255.0, 0xba as f32 / 255.0, 0xcb as f32 / 255.0];
pub const BACK_MODEL: [f32; 3] = [0x5d as f32 / 255.0, 0x71 as f32 / 255.0, 0x86 as f32 / 255.0];

pub(crate) fn hex_rgba(s: &str) -> Option<[u8; 4]> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8, 255])
}

/// A paint (`#rrggbb`, `mat:<id>`, `img:<id>|…`) → its colour, as `paintHex` with the material library's colours
/// registered (modelcraft.js setMaterialColors); a texture shows its average colour when the paint carries one.
fn paint_rgba(c: &str) -> Option<[u8; 4]> {
    let hex = cc_m3d::paint::paint_hex(c, &|id| crate::mcmats::material(id).map(|m| m.color.to_string()), &|_| None);
    hex_rgba(&hex)
}

/// The texture array's layers for imported pictures (`img:` paints): after the library materials' layers.
pub const IMG_LAYER0: usize = cc_m3d_tools::matlib::TEXTURED.len();

/// One imported picture as a texture layer: its image id, a key of its data, its pixels (TEX_SIZE², rows from v = 0).
#[derive(Clone, Debug)]
pub struct ImgTex {
    pub id: String,
    pub key: u64,
    pub rgba: std::sync::Arc<Vec<u8>>,
}

/// The pictures a scene shows as textures, in layer order from [`IMG_LAYER0`].
#[derive(Clone, Debug, Default)]
pub struct ImgLayers {
    pub list: Vec<ImgTex>,
}

impl ImgLayers {
    /// The layer of an image id, if it has one.
    pub fn layer(&self, id: &str) -> Option<f32> {
        self.list.iter().position(|t| t.id == id).map(|k| (IMG_LAYER0 + k) as f32)
    }
    /// (layer, key, pixels) for the renderers.
    pub fn layers(&self) -> Vec<(u32, u64, std::sync::Arc<Vec<u8>>)> {
        self.list.iter().enumerate().map(|(k, t)| ((IMG_LAYER0 + k) as u32, t.key, t.rgba.clone())).collect()
    }
}

/// Decoded pictures kept between scenes: image id → (key of its data, its texture, or None when it cannot be read).
pub type ImgCache = HashMap<String, (u64, Option<std::sync::Arc<Vec<u8>>>)>;

fn data_key(im: &cc_m3d::model::Image) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    im.mime.hash(&mut h);
    im.data.len().hash(&mut h);
    im.data.get(..256.min(im.data.len())).unwrap_or("").hash(&mut h);
    im.data.get(im.data.len().saturating_sub(256)..).unwrap_or("").hash(&mut h);
    h.finish()
}

/// A picture's pixels resampled to the texture layer's square (box filter), rows flipped so v = 0 is the bottom row,
/// as three.js uploads an Image (flipY).
pub fn picture_layer(w: u32, h: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let n = cc_render::mesh3d::TEX_SIZE as usize;
    let (w, h) = (w as usize, h as usize);
    if w == 0 || h == 0 || rgba.len() < w * h * 4 {
        return None;
    }
    let mut out = vec![0u8; n * n * 4];
    for ty in 0..n {
        // texel row ty (v = ty / n) samples the picture's row from the bottom
        let (y0, y1) = ((n - 1 - ty) * h / n, ((n - ty) * h).div_ceil(n).max((n - 1 - ty) * h / n + 1).min(h));
        for tx in 0..n {
            let (x0, x1) = (tx * w / n, ((tx + 1) * w).div_ceil(n).max(tx * w / n + 1).min(w));
            let mut s = [0u32; 4];
            let mut cnt = 0u32;
            for y in y0..y1 {
                for x in x0..x1 {
                    let o = (y * w + x) * 4;
                    if let Some(p) = rgba.get(o..o + 4) {
                        for (a, b) in s.iter_mut().zip(p) {
                            *a += u32::from(*b);
                        }
                        cnt += 1;
                    }
                }
            }
            let cnt = cnt.max(1);
            if let Some(d) = out.get_mut((ty * n + tx) * 4..(ty * n + tx) * 4 + 4) {
                for (a, b) in d.iter_mut().zip(s) {
                    *a = ((b + cnt / 2) / cnt) as u8;
                }
            }
        }
    }
    Some(out)
}

/// The pictures the shown model paints with (faces and placements), first seen first, each decoded once (`cache`);
/// as many as the texture array has room for (the rest show their average colour).
pub fn img_layers(model: &Model, cache: &mut ImgCache) -> ImgLayers {
    if model.images.is_empty() {
        return ImgLayers::default();
    }
    let mut ids: Vec<String> = Vec::new();
    let mut see = |c: Option<&str>| {
        if let Some(c) = c.filter(|c| cc_m3d::paint::is_image_paint(c)) {
            let id = cc_m3d::paint::image_id(c);
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    };
    for d in model.defs.values() {
        for (_, f) in d.faces.iter() {
            see(f.color.as_deref());
        }
        for (_, i) in d.insts.iter() {
            see(i.color.as_deref());
        }
    }
    let room = (cc_render::mesh3d::TEX_LAYERS as usize).saturating_sub(IMG_LAYER0);
    let mut list = Vec::new();
    for id in ids {
        if list.len() >= room {
            break;
        }
        let Some(im) = model.images.get(&id) else { continue };
        let key = data_key(im);
        let fresh = cache.get(&id).is_none_or(|(k, _)| *k != key);
        if fresh {
            let url = format!("data:{};base64,{}", im.mime, im.data);
            let px = cc_io_markup::picture::decode_rgba(&url).and_then(|(w, h, p)| picture_layer(w, h, &p)).map(std::sync::Arc::new);
            cache.insert(id.clone(), (key, px));
        }
        if let Some((_, Some(px))) = cache.get(&id) {
            list.push(ImgTex { id, key, rgba: px.clone() });
        }
    }
    ImgLayers { list }
}

fn grow(lo: &mut [f64; 3], hi: &mut [f64; 3], p: [f64; 3]) {
    for ((l, h), v) in lo.iter_mut().zip(hi.iter_mut()).zip(p) {
        *l = l.min(v);
        *h = h.max(v);
    }
}

fn rel(p: [f64; 3], o: [f64; 3]) -> [f32; 3] {
    [(p[0] - o[0]) as f32, (p[1] - o[1]) as f32, (p[2] - o[2]) as f32]
}

/// One definition's faces and edges as a mesh; faces without paint take `dflt` (a placement's paint) or `front`.
pub fn def_mesh(def: &Def, dflt: Option<&str>, front: [u8; 4], imgs: &ImgLayers) -> MeshData {
    def_mesh_with(def, dflt, front, imgs, &mut |_| {})
}

/// [`def_mesh`], telling `per_vertex` each vertex's paint (the face's own, else the placement's) as it is stored.
pub fn def_mesh_with(
    def: &Def,
    dflt: Option<&str>,
    front: [u8; 4],
    imgs: &ImgLayers,
    per_vertex: &mut dyn FnMut(Option<&str>),
) -> MeshData {
    let fill = dflt.and_then(paint_rgba).unwrap_or(front);
    let smooth = cc_m3d::edges::smooth_normals(def);
    // the definition's own origin: the middle of its vertices' box (positions are f32 relative to it)
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for (_, p) in def.verts.iter() {
        grow(&mut lo, &mut hi, *p);
    }
    let origin = if lo[0].is_finite() { [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0] } else { [0.0; 3] };
    let mut m = MeshData { origin, ..MeshData::default() };
    for (fid, f) in def.faces.iter() {
        if f.hidden {
            continue;
        }
        let n = face_normal(def, f);
        let c = f.color.as_deref().filter(|c| !c.is_empty()).and_then(paint_rgba).unwrap_or(fill);
        // a library material with a texture (view3d.js: the face's paint, else its placement's): planarUV in metres over
        // the tile's size, less a whole number of tiles per face (f32 keeps the fraction far from the origin)
        let paint = f.color.as_deref().filter(|c| !c.is_empty()).or(dflt);
        // an imported picture (view3d.js: 'img:<id>'): UVs from the paint's own placement (imageUV), in pictures
        let tex = paint.and_then(|p| {
            if cc_m3d::paint::is_image_paint(p) {
                return Some((imgs.layer(&cc_m3d::paint::image_id(p))?, [0.0, 0.0]));
            }
            let id = p.strip_prefix("mat:")?;
            let layer = cc_m3d_tools::matlib::TEXTURED.iter().position(|x| *x == id)?;
            Some((layer as f32, cc_m3d_tools::matlib::tex_spec(id)?.size))
        });
        let mut tile0: Option<[f64; 2]> = None;
        // a face's corners are shared by its triangles (same normal and colour): each is stored once
        let mut at: HashMap<u64, u32> = HashMap::new();
        for t in face_tris(def, f) {
            for v in t {
                let j = match at.get(&v.to_bits()) {
                    Some(j) => *j,
                    None => {
                        let j = m.positions.len() as u32;
                        at.insert(v.to_bits(), j);
                        let mut key = cc_m3d::js::num_str(fid);
                        key.push('|');
                        cc_m3d::js::push_num(&mut key, v);
                        let q = smooth.get(&key).copied().unwrap_or(n);
                        m.positions.push(rel(def.v(v), origin));
                        m.normals.push([q[0] as f32, q[1] as f32, q[2] as f32]);
                        m.colors.push(c);
                        per_vertex(paint);
                        match tex {
                            Some((layer, size)) => {
                                let (u, w) = if size[0] == 0.0 {
                                    let uv = paint.and_then(|p| cc_m3d::paint::image_uv(p, def.v(v)).ok()).unwrap_or([0.0, 0.0]);
                                    (uv[0], uv[1])
                                } else {
                                    let uv = cc_m3d_tools::matlib::planar_uv(def.v(v), n, [0.0, 0.0, 1.0]);
                                    (uv[0] / size[0], uv[1] / size[1])
                                };
                                let o = *tile0.get_or_insert([u.floor(), w.floor()]);
                                m.tex.resize(m.positions.len() - 1, [0.0, 0.0, -1.0]);
                                m.tex.push([(u - o[0]) as f32, (w - o[1]) as f32, layer]);
                            }
                            None if !m.tex.is_empty() => m.tex.push([0.0, 0.0, -1.0]),
                            None => {}
                        }
                        j
                    }
                };
                m.indices.push(j);
            }
        }
    }
    for (_, e) in def.edges.iter() {
        if e.soft || e.hidden {
            continue;
        }
        m.edges.push(rel(def.v(e.a), origin));
        m.edges.push(rel(def.v(e.b), origin));
    }
    m
}

/// The whole model as a scene: one mesh per (definition, placement colour), one instance per placement shown.
pub fn scene_of(model: &Model, id: u64, dark: bool, cache: &mut ImgCache) -> Scene3D {
    let front = if dark { FRONT_MODEL } else { FRONT_PAPER };
    let imgs = img_layers(model, cache);
    let mut meshes: Vec<MeshData> = Vec::new();
    let mut index: HashMap<(String, String), usize> = HashMap::new();
    let mut instances: Vec<Instance3> = Vec::new();
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    model.walk_shown("root", &IDENT, &mut |def: &Def, m: &[f64; 16], _path: &[f64], color: Option<&str>| {
        if def.faces.is_empty() && def.edges.is_empty() {
            return true;
        }
        let key = (def.id.clone(), color.unwrap_or("").to_string());
        let mi = *index.entry(key).or_insert_with(|| {
            meshes.push(def_mesh(def, color, front, &imgs));
            meshes.len() - 1
        });
        for (_, p) in def.verts.iter() {
            let w = [
                m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
                m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
                m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
            ];
            grow(&mut lo, &mut hi, w);
        }
        instances.push(Instance3 { mesh: mi, m: *m, tint: None, hidden: false });
        true
    });
    if !lo[0].is_finite() {
        // an empty model: a 10 m square around the origin to look at
        lo = [-5.0, -5.0, 0.0];
        hi = [5.0, 5.0, 3.0];
    }
    let mut s = Scene3D::new(id, meshes, instances, lo, hi);
    s.images = imgs.layers();
    s
}

/// A sample model made in code (no file): a small building with a door and two windows pushed in, a porch slab and a
/// row of posts placed as one component — what `view.model.sample` and the web build's `?model` open.
pub fn sample_model() -> Model {
    use cc_m3d::ops::{add_polygon, push_pull, rect_pts};
    let mut model = Model::new();
    let rect = |d: &mut Def, o: [f64; 3], u: [f64; 3], v: [f64; 3], w: f64, h: f64| {
        let _ = add_polygon(d, &rect_pts(o, u, v, w, h), None);
    };
    let face_at = |d: &Def, p: [f64; 3], n: [f64; 3]| -> Option<f64> {
        // the face whose plane holds p and whose normal is n
        d.faces.iter().find_map(|(id, f)| {
            let fnrm = face_normal(d, f);
            let q = d.v(*f.loops.first()?.first()?);
            let on = ((p[0] - q[0]) * fnrm[0] + (p[1] - q[1]) * fnrm[1] + (p[2] - q[2]) * fnrm[2]).abs() < 1e-6;
            let same = (fnrm[0] - n[0]).abs() + (fnrm[1] - n[1]).abs() + (fnrm[2] - n[2]).abs() < 1e-6;
            (on && same && cc_m3d::def::face_area(d, f) < 5.0).then_some(id)
        })
    };
    let _ = model.change("Sample", |m: &mut Model| {
        let d = m.root_mut().ok_or(cc_m3d::error::Error::Type)?;
        // the building: 8 × 6 m, 3 m high
        rect(d, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 8.0, 6.0);
        let first = d.faces.keys().next();
        if let Some(f) = first {
            let _ = push_pull(d, f, 3.0);
        }
        // a door and two windows on the front (y = 0), each pushed 0.25 m in
        for (x, z, w, h) in [(3.5, 0.15, 1.0, 2.1), (1.0, 1.0, 1.4, 1.2), (5.6, 1.0, 1.4, 1.2)] {
            rect(d, [x, 0.0, z], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], w, h);
            if let Some(f) = face_at(d, [x + w / 2.0, 0.0, z + h / 2.0], [0.0, -1.0, 0.0]) {
                let _ = push_pull(d, f, -0.25);
            }
        }
        // a porch slab in front
        rect(d, [2.0, -2.5, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 4.0, 2.5);
        Ok(())
    });
    // a post as a component placed four times along the porch
    let post = model.new_def("component", Some("Post"));
    let _ = model.change("Posts", |m: &mut Model| {
        if let Some(d) = m.defs.get_mut(&post) {
            rect(d, [-0.08, -0.08, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0.16, 0.16);
            let first = d.faces.keys().next();
            if let Some(f) = first {
                let _ = push_pull(d, f, 2.6);
            }
        }
        if let Some(root) = m.root_mut() {
            for x in [2.2, 3.4, 4.6, 5.8] {
                cc_m3d::model::place(root, &post, &cc_m3d::vec::translation([x, -2.3, 0.0]));
            }
        }
        Ok(())
    });
    model
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_face_painted_with_a_material_carries_texture_coordinates() {
        let mut model = Model::new();
        let r = model.change("plate", |m: &mut Model| {
            let d = m.root_mut().ok_or(cc_m3d::error::Error::Type)?;
            cc_m3d::ops::add_polygon(d, &[[0.0, 0.0, 0.0], [1.8, 0.0, 0.0], [1.8, 1.2, 0.0], [0.0, 1.2, 0.0]], None)?;
            for f in d.faces.values_mut() {
                f.color = Some("mat:brick-red".into());
            }
            Ok(())
        });
        assert!(r.is_ok(), "{r:?}");
        let s = scene_of(&model, 1, false, &mut ImgCache::new());
        let m = &s.meshes[0];
        assert_eq!(m.tex.len(), m.positions.len());
        // brick-red is layer 0, its tile 0.9 × 0.6 m: the plate is two tiles each way
        assert!(m.tex.iter().all(|t| t[2] == 0.0));
        let (umax, vmax) = m.tex.iter().fold((0.0f32, 0.0f32), |(a, b), t| (a.max(t[0]), b.max(t[1])));
        assert!((umax - 2.0).abs() < 1e-5 && (vmax - 2.0).abs() < 1e-5, "{umax} {vmax}");
    }

    #[test]
    fn a_pushed_box_becomes_one_mesh_with_its_edges() {
        let mut model = Model::new();
        let r = model.change("box", |m: &mut Model| {
            let d = m.root_mut().ok_or(cc_m3d::error::Error::Type)?;
            cc_m3d::ops::add_polygon(d, &[[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [2.0, 1.0, 0.0], [0.0, 1.0, 0.0]], None)?;
            let first = d.faces.keys().next();
            if let Some(fid) = first {
                let _ = cc_m3d::ops::push_pull(d, fid, 1.5);
            }
            Ok(())
        });
        assert!(r.is_ok(), "{r:?}");
        let s = scene_of(&model, 1, false, &mut ImgCache::new());
        assert_eq!(s.instances.len(), 1);
        let m = &s.meshes[0];
        assert_eq!(m.indices.len() / 3, 12, "six faces, two triangles each");
        assert_eq!(m.edges.len() / 2, 12, "twelve edges");
        assert!((s.max[2] - 1.5).abs() < 1e-9 && s.min[0].abs() < 1e-9);
        assert!(m.colors.iter().all(|c| *c == FRONT_PAPER));
    }

    #[test]
    fn the_sample_model_has_a_building_and_four_posts() {
        let model = sample_model();
        let s = scene_of(&model, 1, false, &mut ImgCache::new());
        assert_eq!(s.instances.len(), 5, "the root and four posts");
        assert_eq!(s.meshes.len(), 2, "one mesh per definition: the posts share theirs");
        assert!(s.meshes[0].indices.len() / 3 > 12, "openings pushed in");
    }
}
