//! site3d-export.js — the 3D site scene (cc-civil `site3d`) as GLB (glTF 2.0 binary) and OBJ.
//!
//! Both are written at true scale (the vertical exaggeration taken back out), in metres (US survey feet →
//! 1200/3937 m), Y up (x → x, z → y, y → −z), around the scene's local origin so the numbers stay small and precise in
//! f32. The origin (project easting, northing, elevation, units) is recorded: GLB `extras` on the scene and the root
//! node, OBJ comments. Byte-identical to the 0.5 writer: the JSON chunk is the JS `JSON.stringify` text.

use cc_civil::js::{JsMath, hypot3, to_fixed};
use cc_civil::project::js_stringify;
use cc_civil::site3d::{Scene, rgb, to_f32};
use cc_civil::tr;
use serde_json::{Map, Value, json};

/// `METERS_PER[units] || 1`.
fn meters_per(units: &str) -> f64 {
    match units {
        "ft" => 1200.0 / 3937.0,
        _ => 1.0,
    }
}

/// Positions → metres, Y up, exaggeration undone; normals turned the same way (re-normalised).
#[allow(clippy::arithmetic_side_effects)]
fn convert(scene: &Scene, p: &[f32], n: Option<&[f32]>) -> (Vec<f32>, Option<Vec<f32>>) {
    let k = meters_per(&scene.units);
    let e = if scene.exag != 0.0 && !scene.exag.is_nan() { scene.exag } else { 1.0 };
    let mut out = Vec::with_capacity(p.len());
    for c in p.as_chunks::<3>().0 {
        {
            let [x, y, z] = c;
            out.push(to_f32(f64::from(*x) * k));
            out.push(to_f32((f64::from(*z) / e) * k));
            out.push(to_f32(-f64::from(*y) * k));
        }
    }
    let n_out = n.map(|n| {
        let mut o = Vec::with_capacity(n.len());
        for c in n.as_chunks::<3>().0 {
            {
                let [x, y, z] = c;
                let (x, y, z) = (f64::from(*x), f64::from(*y), f64::from(*z) * e);
                let l0 = hypot3(x, y, z);
                let l = if l0 != 0.0 && !l0.is_nan() { l0 } else { 1.0 };
                o.push(to_f32(x / l));
                o.push(to_f32(z / l));
                o.push(to_f32(-y / l));
            }
        }
        o
    });
    (out, n_out)
}

fn origin_extras(scene: &Scene) -> Value {
    json!({
        "generator": "CivilCraft",
        "units": if scene.units == "m" { "meters" } else { "US survey feet" },
        "origin": { "easting": scene.origin.x, "northing": scene.origin.y, "elevation": scene.origin.z },
        "note": "Coordinates are meters, Y up, relative to the origin (project easting, northing, elevation in the project units); true scale.",
    })
}

/// sRGB → linear.
fn lin(v: f64) -> f64 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).jpow(2.4) }
}

struct Glb {
    chunks: Vec<u8>,
    buffer_views: Vec<Value>,
    accessors: Vec<Value>,
    materials: Vec<Value>,
}
const ARRAY: u32 = 34962;
const ELEMENT: u32 = 34963;
const FLOAT: u32 = 5126;
const UINT: u32 = 5125;

impl Glb {
    #[allow(clippy::arithmetic_side_effects)] // bounded counters and indices (page numbers, padding, digit math)
    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        let pad = (4 - (bytes.len() % 4)) % 4;
        let off = self.chunks.len();
        self.chunks.extend_from_slice(bytes);
        self.chunks.extend(std::iter::repeat_n(0u8, pad));
        let mut v = Map::new();
        v.insert("buffer".into(), json!(0));
        v.insert("byteOffset".into(), json!(off));
        v.insert("byteLength".into(), json!(bytes.len()));
        if let Some(t) = target {
            v.insert("target".into(), json!(t));
        }
        self.buffer_views.push(Value::Object(v));
        self.buffer_views.len().saturating_sub(1)
    }
    #[allow(clippy::arithmetic_side_effects)] // bounded counters and indices (page numbers, padding, digit math)
    fn acc_f32(&mut self, arr: &[f32], vec3: bool, minmax: bool) -> usize {
        let bytes: Vec<u8> = arr.iter().flat_map(|x| x.to_le_bytes()).collect();
        let bv = self.view(&bytes, Some(ARRAY));
        let n = if vec3 { 3 } else { 1 };
        let mut a = Map::new();
        a.insert("bufferView".into(), json!(bv));
        a.insert("componentType".into(), json!(FLOAT));
        a.insert("count".into(), json!(arr.len() / n));
        a.insert("type".into(), json!(if vec3 { "VEC3" } else { "SCALAR" }));
        if minmax {
            let mut min = [f64::INFINITY; 3];
            let mut max = [f64::NEG_INFINITY; 3];
            for c in arr.as_chunks::<3>().0 {
                for (k, v) in c.iter().enumerate() {
                    let v = f64::from(*v);
                    if let Some(m) = min.get_mut(k)
                        && v < *m
                    {
                        *m = v;
                    }
                    if let Some(m) = max.get_mut(k)
                        && v > *m
                    {
                        *m = v;
                    }
                }
            }
            let fr = |v: [f64; 3]| Value::Array(v.iter().map(|x| cc_civil::js::jnum(f64::from(to_f32(*x)))).collect());
            a.insert("min".into(), fr(min));
            a.insert("max".into(), fr(max));
        }
        self.accessors.push(Value::Object(a));
        self.accessors.len().saturating_sub(1)
    }
    fn acc_u32(&mut self, arr: &[u32]) -> usize {
        let bytes: Vec<u8> = arr.iter().flat_map(|x| x.to_le_bytes()).collect();
        let bv = self.view(&bytes, Some(ELEMENT));
        self.accessors.push(json!({ "bufferView": bv, "componentType": UINT, "count": arr.len(), "type": "SCALAR" }));
        self.accessors.len().saturating_sub(1)
    }
    fn material(&mut self, name: &str, color: Option<&str>, double_sided: bool) -> usize {
        let c = color.map_or([1.0, 1.0, 1.0], rgb);
        let mut m = Map::new();
        m.insert("name".into(), json!(name));
        m.insert(
            "pbrMetallicRoughness".into(),
            json!({ "baseColorFactor": [cc_civil::js::jnum(lin(c[0])), cc_civil::js::jnum(lin(c[1])), cc_civil::js::jnum(lin(c[2])), 1], "metallicFactor": 0, "roughnessFactor": 1 }),
        );
        if double_sided {
            m.insert("doubleSided".into(), json!(true));
        }
        self.materials.push(Value::Object(m));
        self.materials.len().saturating_sub(1)
    }
}

fn u32_of(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// `exportGLB(scene, { name })`.
#[allow(clippy::arithmetic_side_effects)] // bounded counters and indices (page numbers, padding, digit math)
pub fn export_glb(scene: &Scene, name: &str) -> Vec<u8> {
    let mut g = Glb { chunks: Vec::new(), buffer_views: Vec::new(), accessors: Vec::new(), materials: Vec::new() };
    let mut meshes: Vec<Value> = Vec::new();
    let mut nodes: Vec<Value> = Vec::new();
    let mut children: Vec<usize> = Vec::new();
    let node = |nm: &str, mesh: usize, nodes: &mut Vec<Value>, children: &mut Vec<usize>| {
        nodes.push(json!({ "name": nm, "mesh": mesh }));
        children.push(nodes.len().saturating_sub(1));
    };
    for m in &scene.meshes {
        if m.indices.is_empty() {
            continue;
        }
        let (p, n) = convert(scene, &m.positions, Some(&m.normals));
        let c: Vec<f32> = m.colors.iter().map(|v| to_f32(lin(f64::from(*v)))).collect();
        let pos = g.acc_f32(&p, true, true);
        let nor = g.acc_f32(&n.unwrap_or_default(), true, false);
        let col = g.acc_f32(&c, true, false);
        let indices = g.acc_u32(&m.indices);
        let mat = g.material(&m.name, None, true);
        meshes.push(json!({ "name": m.name, "primitives": [{ "attributes": { "POSITION": pos, "NORMAL": nor, "COLOR_0": col }, "indices": indices, "material": mat, "mode": 4 }] }));
        node(&m.name, meshes.len().saturating_sub(1), &mut nodes, &mut children);
    }
    for l in &scene.lines {
        if l.positions.is_empty() {
            continue;
        }
        let (p, _) = convert(scene, &l.positions, None);
        let pos = g.acc_f32(&p, true, true);
        let mat = g.material(&l.name, Some(&l.color), false);
        meshes.push(json!({ "name": l.name, "primitives": [{ "attributes": { "POSITION": pos }, "material": mat, "mode": 1 }] }));
        node(&l.name, meshes.len().saturating_sub(1), &mut nodes, &mut children);
    }
    if let Some(pts) = scene.points.as_ref().filter(|p| !p.positions.is_empty()) {
        let (p, _) = convert(scene, &pts.positions, None);
        let pts_name = tr!("scene.layer.node");
        let pos = g.acc_f32(&p, true, true);
        let mat = g.material(&pts_name, Some(&pts.color), false);
        meshes.push(json!({ "name": pts_name, "primitives": [{ "attributes": { "POSITION": pos }, "material": mat, "mode": 0 }] }));
        node(&pts_name, meshes.len().saturating_sub(1), &mut nodes, &mut children);
    }
    let mut root = Map::new();
    root.insert("name".into(), json!(name));
    if !children.is_empty() {
        root.insert("children".into(), json!(children));
    }
    root.insert("extras".into(), origin_extras(scene));
    nodes.push(Value::Object(root));
    let offset = g.chunks.len();
    let mut j = Map::new();
    j.insert("asset".into(), json!({ "version": "2.0", "generator": "CivilCraft" }));
    j.insert("scene".into(), json!(0));
    j.insert("scenes".into(), json!([{ "name": name, "nodes": [nodes.len().saturating_sub(1)], "extras": origin_extras(scene) }]));
    j.insert("nodes".into(), Value::Array(nodes));
    if !meshes.is_empty() {
        j.insert("meshes".into(), Value::Array(meshes));
        j.insert("materials".into(), Value::Array(g.materials));
        j.insert("accessors".into(), Value::Array(g.accessors));
        j.insert("bufferViews".into(), Value::Array(g.buffer_views));
        j.insert("buffers".into(), json!([{ "byteLength": offset }]));
    }
    let json_bytes = js_stringify(&Value::Object(j)).into_bytes();
    let jpad = (4 - (json_bytes.len() % 4)) % 4;
    let jlen = json_bytes.len().saturating_add(jpad);
    let total = 20usize.saturating_add(jlen).saturating_add(if offset > 0 { offset.saturating_add(8) } else { 0 });
    let mut out: Vec<u8> = Vec::with_capacity(total);
    out.extend_from_slice(&0x4654_6c67u32.to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&u32_of(total).to_le_bytes());
    out.extend_from_slice(&u32_of(jlen).to_le_bytes());
    out.extend_from_slice(&0x4e4f_534au32.to_le_bytes());
    out.extend_from_slice(&json_bytes);
    out.extend(std::iter::repeat_n(0x20u8, jpad));
    if offset > 0 {
        out.extend_from_slice(&u32_of(offset).to_le_bytes());
        out.extend_from_slice(&0x004e_4942u32.to_le_bytes());
        out.extend_from_slice(&g.chunks);
    }
    out
}

/// JS `\s` (whitespace and line terminators).
fn js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}
/// `s.replace(/\s+/g, '_')`.
fn underscore(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for c in s.chars() {
        if js_space(c) {
            if !in_ws {
                out.push('_');
            }
            in_ws = true;
        } else {
            out.push(c);
            in_ws = false;
        }
    }
    out
}

/// `exportOBJ(scene, { name })`.
#[allow(clippy::arithmetic_side_effects)]
pub fn export_obj(scene: &Scene, name: &str) -> String {
    let o = scene.origin;
    let f = |v: f32| {
        let s = to_fixed(f64::from(v), 4);
        if s == "-0.0000" { "0.0000".to_string() } else { s }
    };
    let mut nm = String::new();
    let mut in_nl = false;
    for c in name.chars() {
        if c == '\r' || c == '\n' {
            if !in_nl {
                nm.push(' ');
            }
            in_nl = true;
        } else {
            nm.push(c);
            in_nl = false;
        }
    }
    let ns = cc_civil::js::num_to_string;
    let mut lines: Vec<String> = vec![
        format!("# CivilCraft site — {nm}"),
        format!(
            "# Meters, Y up, true scale, relative to the origin below (Z-up project coordinates, {})",
            if scene.units == "m" { "meters" } else { "US survey feet" }
        ),
        format!("# origin easting {} northing {} elevation {}", ns(o.x), ns(o.y), ns(o.z)),
    ];
    let mut base: usize = 1;
    let mut n_base: usize = 1;
    let push_v = |lines: &mut Vec<String>, p: &[f32]| {
        for c in p.as_chunks::<3>().0 {
            {
                let [x, y, z] = c;
                lines.push(format!("v {} {} {}", f(*x), f(*y), f(*z)));
            }
        }
    };
    for m in &scene.meshes {
        if m.indices.is_empty() {
            continue;
        }
        let (p, n) = convert(scene, &m.positions, Some(&m.normals));
        let n = n.unwrap_or_default();
        lines.push(format!("o {}", underscore(&m.name)));
        push_v(&mut lines, &p);
        for c in n.as_chunks::<3>().0 {
            {
                let [x, y, z] = c;
                lines.push(format!("vn {} {} {}", to_fixed(f64::from(*x), 5), to_fixed(f64::from(*y), 5), to_fixed(f64::from(*z), 5)));
            }
        }
        for t in m.indices.as_chunks::<3>().0 {
            {
                let [a, b, c] = t;
                let (a, b, c) = (*a as usize, *b as usize, *c as usize);
                lines.push(format!("f {}//{} {}//{} {}//{}", a + base, a + n_base, b + base, b + n_base, c + base, c + n_base));
            }
        }
        base += p.len() / 3;
        n_base += n.len() / 3;
    }
    for l in &scene.lines {
        if l.positions.is_empty() {
            continue;
        }
        let (p, _) = convert(scene, &l.positions, None);
        lines.push(format!("o {}", underscore(&l.name)));
        push_v(&mut lines, &p);
        let nv = p.len() / 3;
        let mut i = 0;
        while i < nv {
            lines.push(format!("l {} {}", base + i, base + i + 1));
            i += 2;
        }
        base += nv;
    }
    if let Some(pts) = scene.points.as_ref().filter(|p| !p.positions.is_empty()) {
        let (p, _) = convert(scene, &pts.positions, None);
        lines.push(format!("o {}", underscore(&tr!("scene.layer.node"))));
        push_v(&mut lines, &p);
        for i in 0..p.len() / 3 {
            lines.push(format!("p {}", base + i));
        }
    }
    let mut s = lines.join("\n");
    s.push('\n');
    s
}
