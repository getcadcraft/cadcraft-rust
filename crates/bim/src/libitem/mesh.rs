//! libitem.js meshes: glTF 2.0 (.glb, .gltf with embedded buffers), OBJ and STL → one item, moved so its box is
//! centred in X, its back (max Y) on y = 0 and its bottom on z = 0.

use serde_json::{Map, Value as J};

use super::{LIMIT_TRIANGLES, Read, base_item, clip, convex_hull, msg, n, pts_json, r4, s, suggest_category, unit_mm};
use crate::js::num_str;

/// A mesh read from a file (file units, its own up axis).
pub struct Tri {
    pub positions: Vec<f64>,
    pub indices: Vec<f64>,
    pub name: String,
}

/// `parseInt(t, 10)`
fn parse_int(t: &str) -> f64 {
    let t = crate::js::trim(t);
    let (neg, rest) = match t.as_bytes().first() {
        Some(b'-') => (true, t.get(1..).unwrap_or("")),
        Some(b'+') => (false, t.get(1..).unwrap_or("")),
        _ => (false, t),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return f64::NAN;
    }
    let v = crate::js::str_num(&digits);
    if neg { -v } else { v }
}

/// `readObj(text, notes)`
fn read_obj(text: &str, notes: &mut Vec<String>) -> Option<Tri> {
    let mut v: Vec<f64> = Vec::new();
    let mut idx: Vec<f64> = Vec::new();
    let mut name = String::new();
    let mut skipped = 0usize;
    for raw in text.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        let line = crate::js::trim(raw);
        if line.starts_with("v ") {
            let p: Vec<&str> = line.split_whitespace().collect();
            for k in 1..=3 {
                v.push(p.get(k).map_or(f64::NAN, |x| crate::js::str_num(x)));
            }
        } else if line.starts_with("f ") {
            let nv = v.len() as f64 / 3.0;
            let ids: Vec<f64> = line
                .split_whitespace()
                .skip(1)
                .map(|t| {
                    let k = parse_int(t.split('/').next().unwrap_or(""));
                    if k < 0.0 { nv + k } else { k - 1.0 }
                })
                .collect();
            if ids.iter().any(|k| !(k.is_finite() && k.fract() == 0.0) || *k < 0.0 || *k >= nv) {
                skipped = skipped.saturating_add(1);
                continue;
            }
            let first = ids.first().copied().unwrap_or(0.0);
            for w in ids.windows(2).skip(1) {
                if let [a, b] = w {
                    idx.extend([first, *a, *b]);
                }
            }
            if idx.len() > LIMIT_TRIANGLES.saturating_mul(3).saturating_add(3) {
                break;
            }
        } else if name.is_empty() && (line.starts_with("o ") || line.starts_with("g ")) {
            name = crate::js::trim(line.get(2..).unwrap_or("")).to_string();
        }
    }
    if skipped > 0 {
        notes.push(msg("libitem.mesh.obj_faces_skipped", &[("n", skipped.to_string())]));
    }
    if idx.is_empty() || v.iter().any(|x| !x.is_finite()) {
        notes.push(msg("libitem.mesh.obj_none", &[]));
        return None;
    }
    Some(Tri { positions: v, indices: idx, name })
}

fn u32le(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o.checked_add(4)?).and_then(|x| x.try_into().ok()).map(u32::from_le_bytes)
}
fn f32le(b: &[u8], o: usize) -> Option<f64> {
    b.get(o..o.checked_add(4)?).and_then(|x| x.try_into().ok()).map(|x| f64::from(f32::from_le_bytes(x)))
}

/// `readStl(u8, notes)`: binary or ASCII; corners that are the same point merged.
fn read_stl(u8b: &[u8], notes: &mut Vec<String>) -> Option<Tri> {
    let mut positions: Vec<f64> = Vec::new();
    let mut indices: Vec<f64> = Vec::new();
    let mut seen: std::collections::HashMap<[u64; 3], f64> = std::collections::HashMap::new();
    let mut name = String::new();
    let mut degenerate = 0usize;
    let mut corner = |x: f64, y: f64, z: f64, positions: &mut Vec<f64>| -> f64 {
        let k = [super::nkey(x), super::nkey(y), super::nkey(z)];
        *seen.entry(k).or_insert_with(|| {
            let i = positions.len() as f64 / 3.0;
            positions.extend([x, y, z]);
            i
        })
    };
    let binary = u8b.len() >= 84
        && u32le(u8b, 80).is_some_and(|c| u64::from(c).checked_mul(50).and_then(|x| x.checked_add(84)) == Some(u8b.len() as u64));
    if binary {
        let count = u32le(u8b, 80).unwrap_or(0) as usize;
        for t in 0..count.min(LIMIT_TRIANGLES.saturating_mul(10)) {
            let o = 84usize.saturating_add(t.saturating_mul(50)).saturating_add(12);
            let mut v = [0.0; 3];
            for (k, slot) in v.iter_mut().enumerate() {
                let b = o.saturating_add(k.saturating_mul(12));
                let (Some(x), Some(y), Some(z)) = (f32le(u8b, b), f32le(u8b, b.saturating_add(4)), f32le(u8b, b.saturating_add(8))) else {
                    break;
                };
                *slot = corner(x, y, z, &mut positions);
            }
            if v[0] == v[1] || v[1] == v[2] || v[0] == v[2] {
                degenerate = degenerate.saturating_add(1);
            } else {
                indices.extend(v);
            }
        }
    } else {
        let text = String::from_utf8_lossy(u8b);
        let head = text.trim_start_matches(|c: char| c.is_whitespace());
        let solid = head.strip_prefix("solid").is_some_and(|r| !r.chars().next().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_'));
        if !solid {
            notes.push(msg("libitem.mesh.stl_unreadable", &[]));
            return None;
        }
        let first = head.get(5..).unwrap_or("").trim_start_matches([' ', '\t']);
        let first = first.split(['\r', '\n']).next().unwrap_or("");
        name = crate::js::trim(&clip(first, 120)).to_string();
        static RX: std::sync::OnceLock<Option<regex::Regex>> = std::sync::OnceLock::new();
        let Some(rx) = RX.get_or_init(|| regex::Regex::new(r"vertex\s+(\S+)\s+(\S+)\s+(\S+)|endloop").ok()) else { return None };
        let mut v: Vec<f64> = Vec::new();
        for c in rx.captures_iter(&text) {
            if c.get(0).is_some_and(|m| m.as_str() == "endloop") {
                let first = v.first().copied().unwrap_or(0.0);
                for w in v.windows(2).skip(1) {
                    if let [a, b] = w {
                        if first == *a || *a == *b || first == *b {
                            degenerate = degenerate.saturating_add(1);
                        } else {
                            indices.extend([first, *a, *b]);
                        }
                    }
                }
                v.clear();
                if indices.len() > LIMIT_TRIANGLES.saturating_mul(30) {
                    break;
                }
                continue;
            }
            let g = |i: usize| c.get(i).map_or(f64::NAN, |m| crate::js::str_num(m.as_str()));
            let (x, y, z) = (g(1), g(2), g(3));
            if !(x.is_finite() && y.is_finite() && z.is_finite()) {
                notes.push(msg("libitem.mesh.stl_corner", &[]));
                return None;
            }
            v.push(corner(x, y, z, &mut positions));
        }
    }
    if degenerate > 0 {
        notes.push(msg("libitem.mesh.stl_degenerate", &[("n", degenerate.to_string())]));
    }
    if indices.is_empty() || positions.iter().any(|x| !x.is_finite()) {
        notes.push(msg("libitem.mesh.stl_none", &[]));
        return None;
    }
    Some(Tri { positions, indices, name })
}

type M4 = [f64; 16];
fn mat(node: &Map<String, J>) -> M4 {
    let arr = |k: &str, d: &[f64]| -> Vec<f64> {
        match node.get(k) {
            Some(J::Array(a)) => a.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect(),
            _ => d.to_vec(),
        }
    };
    if let Some(J::Array(mm)) = node.get("matrix") {
        let mut o = [f64::NAN; 16];
        for (slot, v) in o.iter_mut().zip(mm.iter()) {
            *slot = v.as_f64().unwrap_or(f64::NAN);
        }
        return o;
    }
    let t = arr("translation", &[0.0, 0.0, 0.0]);
    let q = arr("rotation", &[0.0, 0.0, 0.0, 1.0]);
    let sc = arr("scale", &[1.0, 1.0, 1.0]);
    let g = |v: &[f64], i: usize| v.get(i).copied().unwrap_or(f64::NAN);
    let (tx, ty, tz) = (g(&t, 0), g(&t, 1), g(&t, 2));
    let (qx, qy, qz, qw) = (g(&q, 0), g(&q, 1), g(&q, 2), g(&q, 3));
    let (sx, sy, sz) = (g(&sc, 0), g(&sc, 1), g(&sc, 2));
    [
        (1.0 - 2.0 * (qy * qy + qz * qz)) * sx,
        2.0 * (qx * qy + qz * qw) * sx,
        2.0 * (qx * qz - qy * qw) * sx,
        0.0,
        2.0 * (qx * qy - qz * qw) * sy,
        (1.0 - 2.0 * (qx * qx + qz * qz)) * sy,
        2.0 * (qy * qz + qx * qw) * sy,
        0.0,
        2.0 * (qx * qz + qy * qw) * sz,
        2.0 * (qy * qz - qx * qw) * sz,
        (1.0 - 2.0 * (qx * qx + qy * qy)) * sz,
        0.0,
        tx,
        ty,
        tz,
        1.0,
    ]
}
fn mul(a: &M4, b: &M4) -> M4 {
    let mut o = [0.0; 16];
    for c in 0..4usize {
        for r in 0..4usize {
            let mut s = 0.0;
            for k in 0..4usize {
                s += a.get(k.saturating_mul(4).saturating_add(r)).copied().unwrap_or(0.0)
                    * b.get(c.saturating_mul(4).saturating_add(k)).copied().unwrap_or(0.0);
            }
            if let Some(x) = o.get_mut(c.saturating_mul(4).saturating_add(r)) {
                *x = s;
            }
        }
    }
    o
}

struct Gltf<'a> {
    json: &'a J,
    buffers: Vec<Option<Vec<u8>>>,
}
impl Gltf<'_> {
    /// `read(ai, comps)`: an accessor's values (None: missing, sparse, or not that shape).
    fn read(&self, ai: Option<&J>, comps: usize) -> Option<Vec<f64>> {
        let ai = usize::try_from(ai?.as_u64()?).ok()?;
        let a = self.json.get("accessors")?.get(ai)?;
        if a.get("sparse").is_some_and(|x| !x.is_null() && x != &J::Bool(false)) {
            return None;
        }
        let bv = self.json.get("bufferViews")?.get(usize::try_from(a.get("bufferView")?.as_u64()?).ok()?)?;
        let buf = self.buffers.get(usize::try_from(bv.get("buffer")?.as_u64()?).ok()?)?.as_ref()?;
        let ct = a.get("componentType")?.as_u64()?;
        let size: usize = match ct {
            5120 | 5121 => 1,
            5122 | 5123 => 2,
            5125 | 5126 => 4,
            _ => return None,
        };
        let nc: usize = match a.get("type")?.as_str()? {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            _ => return None,
        };
        if nc != comps {
            return None;
        }
        let uz = |v: Option<&J>| v.and_then(J::as_u64).and_then(|x| usize::try_from(x).ok()).unwrap_or(0);
        let stride = match uz(bv.get("byteStride")) {
            0 => size.saturating_mul(nc),
            s => s,
        };
        let base = uz(bv.get("byteOffset")).saturating_add(uz(a.get("byteOffset")));
        let count = uz(a.get("count"));
        // a count past the end of its buffer: the JS throws (a RangeError); here the file is unreadable
        let last = base.checked_add(count.checked_sub(1).map_or(0, |c| c.saturating_mul(stride)))?.checked_add(size.saturating_mul(nc))?;
        if count > 0 && last > buf.len() {
            return None;
        }
        let mut out = Vec::with_capacity(count.saturating_mul(nc).min(1 << 26));
        for i in 0..count {
            for c in 0..nc {
                let o = base.saturating_add(i.saturating_mul(stride)).saturating_add(c.saturating_mul(size));
                let b = buf.get(o..o.saturating_add(size))?;
                out.push(match (ct, b) {
                    (5126, [a, b2, c2, d]) => f64::from(f32::from_le_bytes([*a, *b2, *c2, *d])),
                    (5125, [a, b2, c2, d]) => f64::from(u32::from_le_bytes([*a, *b2, *c2, *d])),
                    (5123, [a, b2]) => f64::from(u16::from_le_bytes([*a, *b2])),
                    (5122, [a, b2]) => f64::from(i16::from_le_bytes([*a, *b2])),
                    (5121, [a]) => f64::from(*a),
                    (5120, [a]) => f64::from(i8::from_le_bytes([*a])),
                    _ => return None,
                });
            }
        }
        Some(out)
    }
}

fn base64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len().saturating_mul(3) / 4);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c.wrapping_sub(b'A'),
            b'a'..=b'z' => c.wrapping_sub(b'a').wrapping_add(26),
            b'0'..=b'9' => c.wrapping_sub(b'0').wrapping_add(52),
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' => continue,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits = bits.saturating_add(6);
        if bits >= 8 {
            bits = bits.saturating_sub(8);
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

/// `readGltf(u8, notes)`
fn read_gltf(u8b: &[u8], notes: &mut Vec<String>) -> Option<Tri> {
    let mut json: Option<J> = None;
    let mut bin: Option<Vec<u8>> = None;
    let bad = |notes: &mut Vec<String>| {
        notes.push(msg("libitem.mesh.gltf_unreadable", &[]));
        None
    };
    if u8b.len() >= 12 && u32le(u8b, 0) == Some(0x4654_6c67) {
        if u32le(u8b, 4) != Some(2) {
            notes.push(msg("libitem.mesh.gltf_version", &[]));
            return None;
        }
        let mut off = 12usize;
        while off.saturating_add(8) <= u8b.len() {
            let len = u32le(u8b, off).unwrap_or(0) as usize;
            let ty = u32le(u8b, off.saturating_add(4)).unwrap_or(0);
            let start = off.saturating_add(8);
            let body = u8b.get(start..start.saturating_add(len).min(u8b.len())).unwrap_or(&[]);
            if ty == 0x4e4f_534a {
                match serde_json::from_str::<J>(&String::from_utf8_lossy(body)) {
                    Ok(j) => json = Some(j),
                    Err(_) => return bad(notes),
                }
            } else if ty == 0x004e_4942 {
                bin = Some(body.to_vec());
            }
            off = start.saturating_add(len);
        }
    } else {
        match serde_json::from_str::<J>(&String::from_utf8_lossy(u8b)) {
            Ok(j) => json = Some(j),
            Err(_) => return bad(notes),
        }
    }
    let Some(json) = json.filter(|j| j.get("asset").is_some_and(|a| !a.is_null())) else {
        notes.push(msg("libitem.mesh.gltf_version", &[]));
        return None;
    };
    let ver = super::jstr(json.get("asset").and_then(|a| a.get("version")).filter(|v| !matches!(v, J::Null | J::Bool(false))));
    if ver.split('.').next() != Some("2") {
        notes.push(msg("libitem.mesh.gltf_version", &[]));
        return None;
    }
    if let Some(J::Array(ext)) = json.get("extensionsRequired")
        && let Some(e) = ext.first()
    {
        notes.push(msg("libitem.mesh.gltf_extension", &[("extension", super::jstr(Some(e)))]));
        return None;
    }
    let mut buffers: Vec<Option<Vec<u8>>> = Vec::new();
    if let Some(J::Array(bs)) = json.get("buffers") {
        for (i, b) in bs.iter().enumerate() {
            match b.get("uri") {
                None => buffers.push(if i == 0 { bin.take() } else { None }),
                Some(u) => {
                    let us = super::jstr(Some(u));
                    let data =
                        us.strip_prefix("data:").and_then(|r| r.split_once(',')).filter(|(h, _)| h.ends_with(";base64")).map(|(_, d)| d);
                    match data.filter(|d| !d.contains('\n') && !d.contains('\r')) {
                        Some(d) => match base64(d) {
                            Some(v) => buffers.push(Some(v)),
                            None => return bad(notes),
                        },
                        None => {
                            notes.push(msg("libitem.mesh.gltf_buffer", &[("n", i.to_string()), ("uri", clip(&us, 80))]));
                            buffers.push(None);
                        }
                    }
                }
            }
        }
    }
    let g = Gltf { json: &json, buffers };
    let mut positions: Vec<f64> = Vec::new();
    let mut indices: Vec<f64> = Vec::new();
    let mut skipped = 0usize;
    let mut stack: Vec<(usize, M4, u32)> = Vec::new();
    const I: M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    let scene_i = json.get("scene").and_then(J::as_u64).unwrap_or(0);
    let scene = json.get("scenes").and_then(|s| s.get(usize::try_from(scene_i).unwrap_or(usize::MAX)));
    let roots: Vec<usize> = match scene {
        Some(sc) => sc
            .get("nodes")
            .and_then(J::as_array)
            .map(|a| a.iter().filter_map(|x| x.as_u64().and_then(|v| usize::try_from(v).ok())).collect())
            .unwrap_or_default(),
        None => (0..json.get("nodes").and_then(J::as_array).map_or(0, Vec::len)).collect(),
    };
    // depth-first in the JS's order: a root, its children in order, then the next root
    for r in roots.into_iter().rev() {
        stack.push((r, I, 0));
    }
    let mut visits = 0usize;
    while let Some((ni, parent, depth)) = stack.pop() {
        visits = visits.saturating_add(1);
        if visits > 1_000_000 || positions.len() > LIMIT_TRIANGLES.saturating_mul(30) {
            break;
        }
        let Some(node) = json.get("nodes").and_then(|x| x.get(ni)).and_then(J::as_object) else { continue };
        if depth > 64 {
            continue;
        }
        let mm = mul(&parent, &mat(node));
        let mesh = node
            .get("mesh")
            .and_then(J::as_u64)
            .and_then(|k| json.get("meshes").and_then(|x| x.get(usize::try_from(k).unwrap_or(usize::MAX))));
        for prim in mesh.and_then(|x| x.get("primitives")).and_then(J::as_array).map(Vec::as_slice).unwrap_or(&[]) {
            if prim.get("mode").and_then(J::as_f64).unwrap_or(4.0) != 4.0 {
                skipped = skipped.saturating_add(1);
                continue;
            }
            let Some(pos) = g.read(prim.get("attributes").and_then(|a| a.get("POSITION")), 3) else { continue };
            let base = positions.len() as f64 / 3.0;
            for c in pos.chunks(3) {
                if let [x, y, z] = c {
                    positions.extend([
                        mm[0] * x + mm[4] * y + mm[8] * z + mm[12],
                        mm[1] * x + mm[5] * y + mm[9] * z + mm[13],
                        mm[2] * x + mm[6] * y + mm[10] * z + mm[14],
                    ]);
                }
            }
            let ix = match prim.get("indices") {
                Some(J::Null) | None => Some((0..pos.len() / 3).map(|i| i as f64).collect()),
                Some(x) => g.read(Some(x), 1),
            };
            if let Some(ix) = ix {
                indices.extend(ix.into_iter().map(|k| base + k));
            }
        }
        if let Some(J::Array(ch)) = node.get("children") {
            for c in ch.iter().rev() {
                if let Some(c) = c.as_u64().and_then(|v| usize::try_from(v).ok()) {
                    stack.push((c, mm, depth.saturating_add(1)));
                }
            }
        }
    }
    if skipped > 0 {
        notes.push(msg("libitem.mesh.gltf_lines", &[("n", skipped.to_string())]));
    }
    if indices.is_empty() {
        notes.push(msg("libitem.mesh.gltf_none", &[]));
        return None;
    }
    let nm = scene
        .and_then(|s| s.get("name"))
        .filter(|v| !matches!(v, J::Null) && !super::jstr(Some(v)).is_empty())
        .or_else(|| json.get("meshes").and_then(|m| m.get(0)).and_then(|m| m.get("name")).filter(|v| !matches!(v, J::Null)));
    Some(Tri { positions, indices, name: clip(&super::jstr(nm), 120) })
}

/// `itemFromMesh(bytes, kind, { units, upAxis, name })`
pub fn item_from_mesh(bytes: &[u8], kind: &str, units: Option<&str>, up_axis: Option<&str>, name: &str) -> Read {
    let mut notes: Vec<String> = Vec::new();
    let tri = match kind {
        "obj" => {
            let t = read_obj(&String::from_utf8_lossy(bytes), &mut notes);
            if units.is_none() {
                notes.push(msg("libitem.mesh.obj_units", &[]));
            }
            if up_axis.is_none() {
                notes.push(msg("libitem.mesh.obj_up", &[]));
            }
            t
        }
        "stl" => {
            let t = read_stl(bytes, &mut notes);
            if units.is_none() {
                notes.push(msg("libitem.mesh.stl_units", &[]));
            }
            if up_axis.is_none() {
                notes.push(msg("libitem.mesh.stl_up", &[]));
            }
            t
        }
        _ => {
            let t = read_gltf(bytes, &mut notes);
            if let Some(u) = units.filter(|u| *u != "m") {
                notes.push(msg("libitem.mesh.gltf_units", &[("units", u.to_string())]));
            }
            t
        }
    };
    let Some(tri) = tri else { return Read { items: Vec::new(), notes } };
    let dflt = if kind == "obj" || kind == "stl" { "mm" } else { "m" };
    let scale = unit_mm(units.unwrap_or(dflt)).unwrap_or(1.0);
    let up = up_axis.unwrap_or(if kind == "stl" { "z" } else { "y" });
    let nt = tri.indices.len() as f64 / 3.0;
    if nt > LIMIT_TRIANGLES as f64 {
        notes.push(msg("libitem.mesh.too_many", &[("n", num_str(nt)), ("limit", LIMIT_TRIANGLES.to_string())]));
        return Read { items: Vec::new(), notes };
    }
    let p = &tri.positions;
    let nv = p.len() / 3;
    let mut out = vec![0.0; nv.saturating_mul(3)];
    let (mut min_x, mut min_y, mut min_z) = (f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let (mut max_x, mut max_y, mut max_z) = (f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for (i, c) in p.chunks(3).enumerate() {
        let [x0, y0, z0] =
            [c.first().copied().unwrap_or(f64::NAN), c.get(1).copied().unwrap_or(f64::NAN), c.get(2).copied().unwrap_or(f64::NAN)];
        let (x, mut y, mut z) = (x0 * scale, y0 * scale, z0 * scale);
        if up == "y" {
            let t = y;
            y = -z;
            z = t;
        }
        let b0 = i.saturating_mul(3);
        if let Some(o) = out.get_mut(b0..b0.saturating_add(3)) {
            o.copy_from_slice(&[x, y, z]);
        }
        if x < min_x {
            min_x = x;
        }
        if x > max_x {
            max_x = x;
        }
        if y < min_y {
            min_y = y;
        }
        if y > max_y {
            max_y = y;
        }
        if z < min_z {
            min_z = z;
        }
        if z > max_z {
            max_z = z;
        }
    }
    if !(max_x > min_x) || !(max_y > min_y) {
        notes.push(msg("libitem.mesh.flat", &[]));
        return Read { items: Vec::new(), notes };
    }
    let cx = (min_x + max_x) / 2.0;
    for c in out.chunks_mut(3) {
        if let [x, y, z] = c {
            *x = r4(*x - cx);
            *y = r4(*y - max_y);
            *z = r4(*z - min_z);
        }
    }
    notes.push(msg("libitem.mesh.moved", &[]));
    let nm = if !name.is_empty() {
        name.to_string()
    } else if !tri.name.is_empty() {
        tri.name.clone()
    } else {
        "Mesh item".into()
    };
    let mut it = base_item(&nm);
    let mut body = Map::new();
    body.insert("type".into(), s("mesh"));
    body.insert("positions".into(), J::Array(out.iter().map(|x| n(*x)).collect()));
    body.insert("indices".into(), J::Array(tri.indices.iter().map(|x| n(*x)).collect()));
    it.insert("body".into(), J::Object(body));
    let xy: Vec<[f64; 2]> = out.chunks(3).map(|c| [c.first().copied().unwrap_or(0.0), c.get(1).copied().unwrap_or(0.0)]).collect();
    let hull = convex_hull(&xy);
    let mut ents = Vec::new();
    if hull.len() >= 3 {
        ents.push(serde_json::json!({ "type": "poly", "layer": "EQUIP", "color": null, "pts": pts_json(&hull), "closed": true }));
    }
    it.insert("plan".into(), serde_json::json!({ "layers": [{ "name": "EQUIP", "color": 7 }], "entities": ents }));
    it.insert(
        "params".into(),
        serde_json::json!({ "width": n(r4(max_x - min_x)), "depth": n(r4(max_y - min_y)), "height": n(r4(max_z - min_z)) }),
    );
    let iname = it.get("name").and_then(J::as_str).unwrap_or("").to_string();
    match suggest_category(&iname) {
        Some(c) => {
            it.insert("category".into(), s(c));
            notes.push(msg("libitem.category.suggested_name", &[("category", c.to_string())]));
        }
        None => notes.push(msg("libitem.category.set", &[("category", "other/other".to_string())])),
    }
    Read { items: vec![J::Object(it)], notes }
}
