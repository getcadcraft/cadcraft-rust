//! export.js — files PartCraft writes, no library: binary STL and 3MF (millimetres, Z up), a stored zip, DXF R12 of a
//! sketch, of closed loops and of a sheet-metal flat pattern, and a faceted STEP solid (AP242 or AP214). Byte for byte
//! what the 0.5 JS writes.

use crate::faces::{self, Mesh};
use crate::js;
use crate::jv;
use crate::sketch::{self as s, Ent};
use crate::spline;
use serde_json::Value;
use std::collections::HashMap;

fn z_up(x: f64, y: f64, z: f64) -> [f64; 3] {
    [x, -z, y]
}

/// `Math.fround` then little-endian bytes.
#[allow(clippy::cast_possible_truncation)]
fn f32_le(x: f64) -> [u8; 4] {
    (x as f32).to_le_bytes()
}

/// `stlBinary(mesh, name)`.
pub fn stl_binary(m: &Mesh, name: &str) -> Vec<u8> {
    let nt = m.tri_count();
    let mut out = vec![0u8; 84];
    let head = js::slice16(&format!("PartCraft binary STL {name}"), 79);
    for (i, u) in head.encode_utf16().enumerate() {
        if let Some(b) = out.get_mut(i) {
            *b = u8::try_from(u & 0x7f).unwrap_or(0);
        }
    }
    let n32 = u32::try_from(nt).unwrap_or(u32::MAX).to_le_bytes();
    for (i, b) in n32.iter().enumerate() {
        if let Some(x) = out.get_mut(80usize.saturating_add(i)) {
            *x = *b;
        }
    }
    out.reserve(nt.saturating_mul(50));
    for t in 0..nt {
        let v: Vec<[f64; 3]> = (0..3)
            .map(|k| {
                let p = m.point(m.corner(t, k));
                z_up(p[0], p[1], p[2])
            })
            .collect();
        let g = |i: usize| v.get(i).copied().unwrap_or([f64::NAN; 3]);
        let (v0, v1, v2) = (g(0), g(1), g(2));
        let u = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
        let w = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
        let n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
        let l0 = js::hypot3(n[0], n[1], n[2]);
        let l = if l0 == 0.0 || l0.is_nan() { 1.0 } else { l0 };
        for c in n {
            out.extend_from_slice(&f32_le(c / l));
        }
        for p in [v0, v1, v2] {
            for c in p {
                out.extend_from_slice(&f32_le(c));
            }
        }
        out.extend_from_slice(&[0, 0]);
    }
    out
}

fn crc_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    for (n, slot) in t.iter_mut().enumerate() {
        let mut c = u32::try_from(n).unwrap_or(0);
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *slot = c;
    }
    t
}
/// `crc32(bytes)`.
pub fn crc32(bytes: &[u8]) -> u32 {
    let t = crc_table();
    let mut c: u32 = 0xffff_ffff;
    for &b in bytes {
        let i = usize::from(u8::try_from((c ^ u32::from(b)) & 0xff).unwrap_or(0));
        c = t.get(i).copied().unwrap_or(0) ^ (c >> 8);
    }
    c ^ 0xffff_ffff
}

fn u16le(v: &mut Vec<u8>, x: usize) {
    v.extend_from_slice(&u16::try_from(x & 0xffff).unwrap_or(0).to_le_bytes());
}
fn u32le(v: &mut Vec<u8>, x: usize) {
    v.extend_from_slice(&u32::try_from(x & 0xffff_ffff).unwrap_or(0).to_le_bytes());
}

/// `zipStored(files)`.
pub fn zip_stored(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut parts: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    let mut offset: usize = 0;
    for (name, data) in files {
        let nb = name.as_bytes();
        let crc = usize::try_from(crc32(data)).unwrap_or(0);
        let mut lh = Vec::with_capacity(30);
        u32le(&mut lh, 0x0403_4b50);
        u16le(&mut lh, 20);
        u16le(&mut lh, 0x0800);
        u16le(&mut lh, 0);
        u16le(&mut lh, 0);
        u16le(&mut lh, 0x21);
        u32le(&mut lh, crc);
        u32le(&mut lh, data.len());
        u32le(&mut lh, data.len());
        u16le(&mut lh, nb.len());
        u16le(&mut lh, 0);
        parts.extend_from_slice(&lh);
        parts.extend_from_slice(nb);
        parts.extend_from_slice(data);
        let mut ch = Vec::with_capacity(46);
        u32le(&mut ch, 0x0201_4b50);
        u16le(&mut ch, 20);
        u16le(&mut ch, 20);
        u16le(&mut ch, 0x0800);
        u16le(&mut ch, 0);
        u16le(&mut ch, 0);
        u16le(&mut ch, 0x21);
        u32le(&mut ch, crc);
        u32le(&mut ch, data.len());
        u32le(&mut ch, data.len());
        u16le(&mut ch, nb.len());
        u16le(&mut ch, 0);
        u16le(&mut ch, 0);
        u16le(&mut ch, 0);
        u16le(&mut ch, 0);
        u32le(&mut ch, 0);
        u32le(&mut ch, offset);
        central.extend_from_slice(&ch);
        central.extend_from_slice(nb);
        offset = offset.saturating_add(30).saturating_add(nb.len()).saturating_add(data.len());
    }
    let csize = central.len();
    let mut end = Vec::with_capacity(22);
    u32le(&mut end, 0x0605_4b50);
    u16le(&mut end, 0);
    u16le(&mut end, 0);
    u16le(&mut end, files.len());
    u16le(&mut end, files.len());
    u32le(&mut end, csize);
    u32le(&mut end, offset);
    u16le(&mut end, 0);
    let mut out = parts;
    out.extend_from_slice(&central);
    out.extend_from_slice(&end);
    out
}

/// `xmlEsc(s)`.
pub fn xml_esc(st: &str) -> String {
    let mut out = String::with_capacity(st.len());
    for c in st.chars() {
        match c {
            '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' => {}
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}
fn num9(v: f64) -> String {
    let r = if v.abs() < 1e-12 { 0.0 } else { v };
    js::num_str(js::prec_num(r, 9))
}

/// `threeMf(mesh, name)`.
pub fn three_mf(m: &Mesh, name: &str) -> Vec<u8> {
    use std::fmt::Write;
    let nv = m.positions.len() / 3;
    let mut o = String::new();
    o.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<model unit=\"millimeter\" xml:lang=\"en-US\" xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">\n");
    let nm = xml_esc(name);
    let _ = write!(o, " <metadata name=\"Title\">{nm}</metadata>\n <metadata name=\"Application\">PartCraft</metadata>\n");
    let _ = write!(o, " <resources>\n  <object id=\"1\" type=\"model\" name=\"{nm}\">\n   <mesh>\n    <vertices>\n");
    for i in 0..nv {
        let p = m.point(i);
        let [x, y, z] = z_up(p[0], p[1], p[2]);
        let _ = writeln!(o, "     <vertex x=\"{}\" y=\"{}\" z=\"{}\"/>", num9(x), num9(y), num9(z));
    }
    o.push_str("    </vertices>\n    <triangles>\n");
    for t in 0..m.tri_count() {
        let g = |k: usize| m.indices.get(t.saturating_mul(3).saturating_add(k)).map_or_else(|| "undefined".to_string(), u32::to_string);
        let _ = writeln!(o, "     <triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"/>", g(0), g(1), g(2));
    }
    o.push_str("    </triangles>\n   </mesh>\n  </object>\n </resources>\n <build>\n  <item objectid=\"1\"/>\n </build>\n</model>\n");
    let types = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/></Types>\n";
    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Target=\"/3D/3dmodel.model\" Id=\"rel0\" Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/></Relationships>\n";
    zip_stored(&[
        ("[Content_Types].xml".to_string(), types.as_bytes().to_vec()),
        ("_rels/.rels".to_string(), rels.as_bytes().to_vec()),
        ("3D/3dmodel.model".to_string(), o.into_bytes()),
    ])
}

// ---------------------------------------------------------------- DXF R12

struct Dxf {
    lines: Vec<String>,
}
impl Dxf {
    fn w(&mut self, code: i32, v: impl std::fmt::Display) {
        self.lines.push(format!("{code:>3}"));
        self.lines.push(v.to_string());
    }
    fn text(&self) -> String {
        format!("{}\r\n", self.lines.join("\r\n"))
    }
}
fn ff(n: f64) -> String {
    if n.abs() < 1e-12 { "0.0".into() } else { js::num_str(js::prec_num(n, 15)) }
}

fn dxf_head(d: &mut Dxf, layers: &[(&str, i32, Option<&str>)], ext: Option<[f64; 4]>, dashed: bool) {
    d.w(0, "SECTION");
    d.w(2, "HEADER");
    d.w(9, "$ACADVER");
    d.w(1, "AC1009");
    d.w(9, "$INSBASE");
    d.w(10, "0.0");
    d.w(20, "0.0");
    d.w(30, "0.0");
    if let Some([x0, y0, x1, y1]) = ext {
        d.w(9, "$EXTMIN");
        d.w(10, ff(x0));
        d.w(20, ff(y0));
        d.w(30, "0.0");
        d.w(9, "$EXTMAX");
        d.w(10, ff(x1));
        d.w(20, ff(y1));
        d.w(30, "0.0");
    }
    d.w(9, "$INSUNITS");
    d.w(70, 4);
    d.w(0, "ENDSEC");
    d.w(0, "SECTION");
    d.w(2, "TABLES");
    d.w(0, "TABLE");
    d.w(2, "LTYPE");
    d.w(70, if dashed { 2 } else { 1 });
    d.w(0, "LTYPE");
    d.w(2, "CONTINUOUS");
    d.w(70, 0);
    d.w(3, "Solid line");
    d.w(72, 65);
    d.w(73, 0);
    d.w(40, "0.0");
    if dashed {
        d.w(0, "LTYPE");
        d.w(2, "DASHED");
        d.w(70, 0);
        d.w(3, "__ __ __");
        d.w(72, 65);
        d.w(73, 2);
        d.w(40, "9.0");
        d.w(49, "6.0");
        d.w(49, "-3.0");
    }
    d.w(0, "ENDTAB");
    d.w(0, "TABLE");
    d.w(2, "LAYER");
    d.w(70, layers.len());
    for (name, color, lt) in layers {
        d.w(0, "LAYER");
        d.w(2, name);
        d.w(70, 0);
        d.w(62, color);
        d.w(6, lt.unwrap_or("CONTINUOUS"));
    }
    d.w(0, "ENDTAB");
    d.w(0, "TABLE");
    d.w(2, "STYLE");
    d.w(70, 1);
    d.w(0, "STYLE");
    d.w(2, "STANDARD");
    d.w(70, 0);
    d.w(40, "0.0");
    d.w(41, "1.0");
    d.w(50, "0.0");
    d.w(71, 0);
    d.w(42, "2.5");
    d.w(3, "txt");
    d.w(4, "");
    d.w(0, "ENDTAB");
    d.w(0, "ENDSEC");
    d.w(0, "SECTION");
    d.w(2, "BLOCKS");
    d.w(0, "ENDSEC");
    d.w(0, "SECTION");
    d.w(2, "ENTITIES");
}

pub const SPLINE_DXF_TOL: f64 = 0.01;

/// `dxfSketch(sk)` → (text, counts JSON).
pub fn dxf_sketch(sk: &Value) -> (String, Value) {
    let mut d = Dxf { lines: Vec::new() };
    dxf_head(&mut d, &[("0", 7, None), ("PROFILE", 7, None), ("CONSTRUCTION", 8, None), ("POINTS", 3, None)], None, false);
    let (mut cl, mut ca, mut cc, mut cp, mut cs) = (0usize, 0usize, 0usize, 0usize, 0usize);
    let loose: std::collections::HashSet<String> = s::loose_points(sk).iter().map(|p| js::str_of(p.get("id"))).collect();
    for e in s::entities(sk) {
        let constr = jv::truthy(e.get("construction"));
        let layer = if constr { "CONSTRUCTION" } else { "PROFILE" };
        let en = Ent::E(e);
        match jv::s(e, "type") {
            "line" => {
                let a = s::pxy(sk, &js::str_of(e.get("p1")));
                let b = s::pxy(sk, &js::str_of(e.get("p2")));
                d.w(0, "LINE");
                d.w(8, layer);
                d.w(10, ff(a[0]));
                d.w(20, ff(a[1]));
                d.w(30, "0.0");
                d.w(11, ff(b[0]));
                d.w(21, ff(b[1]));
                d.w(31, "0.0");
                cl = cl.saturating_add(1);
            }
            "circle" => {
                let c = s::pxy(sk, &js::str_of(e.get("c")));
                d.w(0, "CIRCLE");
                d.w(8, layer);
                d.w(10, ff(c[0]));
                d.w(20, ff(c[1]));
                d.w(30, "0.0");
                d.w(40, ff(jv::as_num(e.get("r")).unwrap_or(f64::NAN)));
                cc = cc.saturating_add(1);
            }
            "arc" => {
                let (c, r, a0, sweep) = s::arc_angles(sk, &en);
                let d0 = a0 * 180.0 / std::f64::consts::PI;
                let d1 = (a0 + sweep) * 180.0 / std::f64::consts::PI;
                let norm = |x: f64| ((x % 360.0) + 360.0) % 360.0;
                d.w(0, "ARC");
                d.w(8, layer);
                d.w(10, ff(c[0]));
                d.w(20, ff(c[1]));
                d.w(30, "0.0");
                d.w(40, ff(r));
                d.w(50, ff(norm(d0)));
                d.w(51, ff(norm(d1)));
                ca = ca.saturating_add(1);
            }
            "spline" => {
                let Ok(curve) = s::spline_curve(sk, &en) else { continue };
                let mut pts = spline::tessellate_tol(&curve, SPLINE_DXF_TOL);
                let closed = jv::truthy(e.get("closed"));
                if closed {
                    pts.pop();
                }
                d.w(0, "POLYLINE");
                d.w(8, layer);
                d.w(66, 1);
                d.w(10, "0.0");
                d.w(20, "0.0");
                d.w(30, "0.0");
                d.w(70, i32::from(closed));
                for [x, y] in pts {
                    d.w(0, "VERTEX");
                    d.w(8, layer);
                    d.w(10, ff(x));
                    d.w(20, ff(y));
                    d.w(30, "0.0");
                }
                d.w(0, "SEQEND");
                d.w(8, layer);
                cs = cs.saturating_add(1);
            }
            "point" if loose.contains(&js::str_of(e.get("id"))) => {
                d.w(0, "POINT");
                d.w(8, if constr { "CONSTRUCTION" } else { "POINTS" });
                d.w(10, ff(jv::as_num(e.get("x")).unwrap_or(f64::NAN)));
                d.w(20, ff(jv::as_num(e.get("y")).unwrap_or(f64::NAN)));
                d.w(30, "0.0");
                cp = cp.saturating_add(1);
            }
            _ => {}
        }
    }
    d.w(0, "ENDSEC");
    d.w(0, "EOF");
    let counts = jv::obj(vec![
        ("line", jv::unum(cl)),
        ("arc", jv::unum(ca)),
        ("circle", jv::unum(cc)),
        ("point", jv::unum(cp)),
        ("spline", jv::unum(cs)),
    ]);
    (d.text(), counts)
}

/// `dxfLoops(loops)`.
pub fn dxf_loops(loops: &[Vec<[f64; 2]>]) -> (String, Value) {
    let mut d = Dxf { lines: Vec::new() };
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for l in loops {
        for &[x, y] in l {
            x0 = js::min(x0, x);
            y0 = js::min(y0, y);
            x1 = js::max(x1, x);
            y1 = js::max(y1, y);
        }
    }
    dxf_head(&mut d, &[("0", 7, None), ("PROFILE", 7, None)], if loops.is_empty() { None } else { Some([x0, y0, x1, y1]) }, false);
    for l in loops {
        d.w(0, "POLYLINE");
        d.w(8, "PROFILE");
        d.w(66, 1);
        d.w(10, "0.0");
        d.w(20, "0.0");
        d.w(30, "0.0");
        d.w(70, 1);
        for &[x, y] in l {
            d.w(0, "VERTEX");
            d.w(8, "PROFILE");
            d.w(10, ff(x));
            d.w(20, ff(y));
            d.w(30, "0.0");
        }
        d.w(0, "SEQEND");
        d.w(8, "PROFILE");
    }
    d.w(0, "ENDSEC");
    d.w(0, "EOF");
    (d.text(), jv::obj(vec![("polyline", jv::unum(loops.len()))]))
}

fn p2(v: Option<&Value>) -> [f64; 2] {
    let a = jv::arr(v);
    [jv::as_num(a.first()).unwrap_or(f64::NAN), jv::as_num(a.get(1)).unwrap_or(f64::NAN)]
}

/// `dxfFlat(flat)` with the default labels.
pub fn dxf_flat(flat: &Value) -> (String, Value) {
    let mut d = Dxf { lines: Vec::new() };
    let bx = flat.get("box");
    let g = |k: &str| jv::as_num(bx.and_then(|b| b.get(k))).unwrap_or(f64::NAN);
    let ext = bx.filter(|b| jv::truthy(Some(b))).map(|_| [g("x0"), g("y0"), g("x1"), g("y1")]);
    dxf_head(&mut d, &[("0", 7, None), ("OUTLINE", 7, None), ("BEND", 1, Some("DASHED")), ("BEND_NOTES", 1, None)], ext, true);
    let (mut cl, mut ca, mut cc, mut cpoly, mut cb, mut ct) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    for sg in jv::arr(flat.get("segs")) {
        let n = |k: &str| jv::as_num(sg.get(k)).unwrap_or(f64::NAN);
        match jv::s(sg, "type") {
            "line" => {
                let (a, b) = (p2(sg.get("a")), p2(sg.get("b")));
                d.w(0, "LINE");
                d.w(8, "OUTLINE");
                d.w(10, ff(a[0]));
                d.w(20, ff(a[1]));
                d.w(30, "0.0");
                d.w(11, ff(b[0]));
                d.w(21, ff(b[1]));
                d.w(31, "0.0");
                cl = cl.saturating_add(1);
            }
            "circle" => {
                let c = p2(sg.get("c"));
                d.w(0, "CIRCLE");
                d.w(8, "OUTLINE");
                d.w(10, ff(c[0]));
                d.w(20, ff(c[1]));
                d.w(30, "0.0");
                d.w(40, ff(n("r")));
                cc = cc.saturating_add(1);
            }
            "arc" => {
                let c = p2(sg.get("c"));
                d.w(0, "ARC");
                d.w(8, "OUTLINE");
                d.w(10, ff(c[0]));
                d.w(20, ff(c[1]));
                d.w(30, "0.0");
                d.w(40, ff(n("r")));
                d.w(50, ff(n("a0")));
                d.w(51, ff(n("a1")));
                ca = ca.saturating_add(1);
            }
            "poly" => {
                d.w(0, "POLYLINE");
                d.w(8, "OUTLINE");
                d.w(66, 1);
                d.w(10, "0.0");
                d.w(20, "0.0");
                d.w(30, "0.0");
                d.w(70, 0);
                for p in jv::arr(sg.get("pts")) {
                    let q = p2(Some(p));
                    d.w(0, "VERTEX");
                    d.w(8, "OUTLINE");
                    d.w(10, ff(q[0]));
                    d.w(20, ff(q[1]));
                    d.w(30, "0.0");
                }
                d.w(0, "SEQEND");
                d.w(8, "OUTLINE");
                cpoly = cpoly.saturating_add(1);
            }
            _ => {}
        }
    }
    let h = js::max(1.5, js::min(5.0, 0.02 * js::max(g("x1") - g("x0"), g("y1") - g("y0"))));
    for b in jv::arr(flat.get("bends")) {
        let (a, bb) = (p2(b.get("a")), p2(b.get("b")));
        d.w(0, "LINE");
        d.w(8, "BEND");
        d.w(10, ff(a[0]));
        d.w(20, ff(a[1]));
        d.w(30, "0.0");
        d.w(11, ff(bb[0]));
        d.w(21, ff(bb[1]));
        d.w(31, "0.0");
        cb = cb.saturating_add(1);
        let pi = std::f64::consts::PI;
        let mut ang = js::atan2(bb[1] - a[1], bb[0] - a[0]);
        if ang > pi / 2.0 + 1e-9 || ang <= -pi / 2.0 + 1e-9 {
            ang += pi;
        }
        let m = [(a[0] + bb[0]) / 2.0 - js::sin(ang) * h * 0.6, (a[1] + bb[1]) / 2.0 + js::cos(ang) * h * 0.6];
        let up = jv::truthy(b.get("up"));
        let label = format!(
            "{} {}%%d R{}",
            if up { "UP" } else { "DOWN" },
            js::num_str(js::fixed_num(jv::as_num(b.get("angle")).unwrap_or(f64::NAN), 2)),
            js::num_str(js::fixed_num(jv::as_num(b.get("r")).unwrap_or(f64::NAN), 3))
        );
        d.w(0, "TEXT");
        d.w(8, "BEND_NOTES");
        d.w(10, ff(m[0]));
        d.w(20, ff(m[1]));
        d.w(30, "0.0");
        d.w(40, ff(h));
        d.w(1, label.replace(['\r', '\n'], " "));
        d.w(50, ff(ang * 180.0 / pi));
        d.w(72, 1);
        d.w(11, ff(m[0]));
        d.w(21, ff(m[1]));
        d.w(31, "0.0");
        ct = ct.saturating_add(1);
    }
    d.w(0, "ENDSEC");
    d.w(0, "EOF");
    let counts = jv::obj(vec![
        ("line", jv::unum(cl)),
        ("arc", jv::unum(ca)),
        ("circle", jv::unum(cc)),
        ("polyline", jv::unum(cpoly)),
        ("bend", jv::unum(cb)),
        ("text", jv::unum(ct)),
    ]);
    (d.text(), counts)
}

// ---------------------------------------------------------------- STEP (faceted)

/// `stepReal(v)`.
pub fn step_real(v: f64) -> String {
    if !v.is_finite() || v.abs() < 1e-15 {
        return "0.".into();
    }
    // Number(v.toPrecision(15)).toExponential(): the shortest digits of the rounded value
    let e = format!("{:e}", js::prec_num(v, 15)).to_uppercase();
    let (m, ex) = e.split_once('E').unwrap_or((e.as_str(), "0"));
    let mut m = m.to_string();
    if !m.contains('.') {
        m.push('.');
    }
    let exn: i64 = ex.parse().unwrap_or(0);
    if exn == 0 {
        return m;
    }
    format!("{m}E{}{:02}", if exn < 0 { "-" } else { "" }, exn.unsigned_abs())
}
fn step_str(st: &str) -> String {
    let mut out = String::from("'");
    for u in st.encode_utf16() {
        let c = if (0x20..=0x7e).contains(&u) { char::from_u32(u32::from(u)).unwrap_or('_') } else { '_' };
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("''"),
            _ => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// `stepFaceted(mesh, name, { date, schema })`; `iso` = `date.toISOString().slice(0, 19)`.
pub fn step_faceted(m: &Mesh, name: &str, iso: &str, ap214: bool) -> (String, Value) {
    let mut ents: Vec<String> = Vec::new();
    let add = |ents: &mut Vec<String>, st: String| -> String {
        ents.push(st);
        format!("#{}", ents.len())
    };
    let nm = step_str(name);
    let ctx = add(
        &mut ents,
        if ap214 {
            "APPLICATION_CONTEXT('core data for automotive mechanical design processes')".into()
        } else {
            "APPLICATION_CONTEXT('managed model based 3d engineering')".into()
        },
    );
    add(
        &mut ents,
        if ap214 {
            format!("APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000,{ctx})")
        } else {
            format!("APPLICATION_PROTOCOL_DEFINITION('international standard','ap242_managed_model_based_3d_engineering',2011,{ctx})")
        },
    );
    let pctx = add(&mut ents, format!("PRODUCT_CONTEXT('',{ctx},'mechanical')"));
    let prod = add(&mut ents, format!("PRODUCT({nm},{nm},'',({pctx}))"));
    add(&mut ents, format!("PRODUCT_RELATED_PRODUCT_CATEGORY('part',$,({prod}))"));
    let pdf = add(&mut ents, format!("PRODUCT_DEFINITION_FORMATION('','',{prod})"));
    let pdc = add(&mut ents, format!("PRODUCT_DEFINITION_CONTEXT('part definition',{ctx},'design')"));
    let pd = add(&mut ents, format!("PRODUCT_DEFINITION('design','',{pdf},{pdc})"));
    let pds = add(&mut ents, format!("PRODUCT_DEFINITION_SHAPE('','',{pd})"));
    let lu = add(&mut ents, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))".into());
    let au = add(&mut ents, "(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.))".into());
    let su = add(&mut ents, "(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT())".into());
    let unc = add(
        &mut ents,
        format!("UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),{lu},'distance_accuracy_value','confusion accuracy')"),
    );
    let gctx = add(
        &mut ents,
        format!(
            "(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT(({unc}))GLOBAL_UNIT_ASSIGNED_CONTEXT(({lu},{au},{su}))REPRESENTATION_CONTEXT('Context #1','3D Context with UNIT and UNCERTAINTY'))"
        ),
    );
    let o0 = add(&mut ents, "CARTESIAN_POINT('',(0.,0.,0.))".into());
    let za = add(&mut ents, "DIRECTION('',(0.,0.,1.))".into());
    let xa = add(&mut ents, "DIRECTION('',(1.,0.,0.))".into());
    let place = add(&mut ents, format!("AXIS2_PLACEMENT_3D('',{o0},{za},{xa})"));
    let mut vref: HashMap<usize, String> = HashMap::new();
    let vp = |ents: &mut Vec<String>, vref: &mut HashMap<usize, String>, v: usize| -> String {
        if let Some(r) = vref.get(&v) {
            return r.clone();
        }
        let p = m.point(v);
        ents.push(format!("CARTESIAN_POINT('',({},{},{}))", step_real(p[0]), step_real(p[1]), step_real(p[2])));
        let r = format!("#{}", ents.len());
        vref.insert(v, r.clone());
        r
    };
    let (_, groups) = faces::planar_groups_mesh(m, 1e-5);
    let mut face_refs: Vec<String> = Vec::new();
    let mut planar_faces = 0usize;
    for g in &groups {
        let mut loops = faces::boundary_loops(m, &g.tris);
        if loops.is_empty() {
            continue;
        }
        let area_of = |l: &[usize]| -> f64 {
            let mut sm = [0.0f64; 3];
            let n = l.len();
            for i in 0..n {
                let a = m.point(l.get(i).copied().unwrap_or(usize::MAX));
                let b = m.point(l.get(i.saturating_add(1).checked_rem(n).unwrap_or(0)).copied().unwrap_or(usize::MAX));
                sm = [sm[0] + a[1] * b[2] - a[2] * b[1], sm[1] + a[2] * b[0] - a[0] * b[2], sm[2] + a[0] * b[1] - a[1] * b[0]];
            }
            (sm[0] * g.n[0] + sm[1] * g.n[1] + sm[2] * g.n[2]) / 2.0
        };
        cc_m3d::sort::timsort(&mut loops, |a, b| area_of(b) - area_of(a));
        let mut bounds = Vec::new();
        for (i, l) in loops.iter().enumerate() {
            let pts: Vec<String> = l.iter().map(|&v| vp(&mut ents, &mut vref, v)).collect();
            let pl = add(&mut ents, format!("POLY_LOOP('',({}))", pts.join(",")));
            bounds.push(add(&mut ents, format!("{}('',{pl},.T.)", if i == 0 { "FACE_OUTER_BOUND" } else { "FACE_BOUND" })));
        }
        let n = g.n;
        let mut xd = if n[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
        let dd = xd[0] * n[0] + xd[1] * n[1] + xd[2] * n[2];
        xd = [xd[0] - dd * n[0], xd[1] - dd * n[1], xd[2] - dd * n[2]];
        let xl = js::hypot3(xd[0], xd[1], xd[2]);
        xd = xd.map(|c| c / xl);
        let v0 = loops.first().and_then(|l| l.first()).copied().unwrap_or(usize::MAX);
        let p0 = vp(&mut ents, &mut vref, v0);
        let dn = add(&mut ents, format!("DIRECTION('',({}))", n.map(step_real).join(",")));
        let dx = add(&mut ents, format!("DIRECTION('',({}))", xd.map(step_real).join(",")));
        let ap = add(&mut ents, format!("AXIS2_PLACEMENT_3D('',{p0},{dn},{dx})"));
        let plane = add(&mut ents, format!("PLANE('',{ap})"));
        face_refs.push(add(&mut ents, format!("FACE_SURFACE('',({}),{plane},.T.)", bounds.join(","))));
        if g.tris.len() > 1 {
            planar_faces = planar_faces.saturating_add(1);
        }
    }
    let shell = add(&mut ents, format!("CLOSED_SHELL('',({}))", face_refs.join(",")));
    let brep = add(&mut ents, format!("FACETED_BREP({nm},{shell})"));
    let rep = add(&mut ents, format!("FACETED_BREP_SHAPE_REPRESENTATION({nm},({place},{brep}),{gctx})"));
    add(&mut ents, format!("SHAPE_DEFINITION_REPRESENTATION({pds},{rep})"));
    let mut lines: Vec<String> = vec![
        "ISO-10303-21;".into(),
        "HEADER;".into(),
        "FILE_DESCRIPTION(('PartCraft faceted solid'),'2;1');".into(),
        format!("FILE_NAME({},'{iso}',(''),(''),'PartCraft','PartCraft','');", step_str(&format!("{name}.step"))),
        if ap214 {
            "FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));".into()
        } else {
            "FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }'));".into()
        },
        "ENDSEC;".into(),
        "DATA;".into(),
    ];
    for (i, e) in ents.iter().enumerate() {
        lines.push(format!("#{}={e};", i.saturating_add(1)));
    }
    lines.push("ENDSEC;".into());
    lines.push("END-ISO-10303-21;".into());
    lines.push(String::new());
    let counts =
        jv::obj(vec![("faces", jv::unum(face_refs.len())), ("planarFaces", jv::unum(planar_faces)), ("points", jv::unum(vref.len()))]);
    (lines.join("\n"), counts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stl_zip_step_and_dxf() {
        let m = Mesh {
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            indices: vec![0, 2, 1, 0, 1, 3, 1, 2, 3, 0, 3, 2],
            tri_face: None,
        };
        let stl = stl_binary(&m, "tet");
        assert_eq!(stl.len(), 84 + 4 * 50);
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        let z = three_mf(&m, "a & b");
        assert_eq!(z.get(..4), Some(&[0x50, 0x4b, 0x03, 0x04][..]));
        let (text, counts) = step_faceted(&m, "tet", "2026-10-17T00:00:00", false);
        assert!(text.starts_with("ISO-10303-21;") && counts.get("faces").and_then(Value::as_u64) == Some(4));
        assert_eq!(step_real(1234.5), "1.2345E03");
        assert_eq!(step_real(0.5), "5.E-01");
        let (dxf, _) = dxf_loops(&[vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]]);
        assert!(dxf.ends_with("EOF\r\n"));
        // hostile: indices past the positions
        let bad = Mesh { positions: vec![0.0; 3], indices: vec![9, 9, 9, 1], tri_face: None };
        let _ = stl_binary(&bad, "");
        let _ = step_faceted(&bad, "", "", true);
        assert_eq!(crate::ifc_export::guid_for("x").len(), 22);
    }
}
