//! bcf.js — BCF 2.1 issues in and out, with a real model camera (0.4 B9): a .bcf zip (stored entries) written
//! byte for byte as the JS writes it, any BCF 2.0 / 2.1 zip read (stored or deflated) as untrusted input — sizes,
//! depth and counts capped, unknown parts ignored, the same notes and error texts — and the camera conversions between
//! the viewer (Y up, moved by the coordination matrix) and IFC (Z up, metres).

use cc_io_ifc::util::push_json_str16;
use serde_json::Value as J;
use zlib_rs::{Inflate, InflateFlush, Status};

use crate::js::{OMap, hypot3, num_str, round};
use crate::xml::{Doc, lossy, parse_xml, u16s};

pub const MAX_FILE: u64 = 20 * 1024 * 1024;
pub const MAX_TOTAL: u64 = 200 * 1024 * 1024;
pub const MAX_ENTRIES: u16 = 5000;
pub const MAX_TOPICS: usize = 2000;
/// Inflated bytes past which a stream is not followed to its end (the JS would run out of memory first).
const MAX_INFLATE_WORK: u64 = 1 << 30;

// ---- coordinates ----

type M = [f64; 16];
type V = [f64; 3];

/// bcf.js's own column-major 4 × 4 inverse (a 0 determinant gives zeros, not an error).
#[rustfmt::skip]
pub fn inv4(a: &M) -> M {
    let b00 = a[0] * a[5] - a[1] * a[4]; let b01 = a[0] * a[6] - a[2] * a[4]; let b02 = a[0] * a[7] - a[3] * a[4]; let b03 = a[1] * a[6] - a[2] * a[5];
    let b04 = a[1] * a[7] - a[3] * a[5]; let b05 = a[2] * a[7] - a[3] * a[6]; let b06 = a[8] * a[13] - a[9] * a[12]; let b07 = a[8] * a[14] - a[10] * a[12];
    let b08 = a[8] * a[15] - a[11] * a[12]; let b09 = a[9] * a[14] - a[10] * a[13]; let b10 = a[9] * a[15] - a[11] * a[13]; let b11 = a[10] * a[15] - a[11] * a[14];
    let det = b00 * b11 - b01 * b10 + b02 * b09 + b03 * b08 - b04 * b07 + b05 * b06;
    let d = if det == 0.0 || det.is_nan() { 0.0 } else { 1.0 / det };
    [
        (a[5] * b11 - a[6] * b10 + a[7] * b09) * d, (a[2] * b10 - a[1] * b11 - a[3] * b09) * d, (a[13] * b05 - a[14] * b04 + a[15] * b03) * d, (a[10] * b04 - a[9] * b05 - a[11] * b03) * d,
        (a[6] * b08 - a[4] * b11 - a[7] * b07) * d, (a[0] * b11 - a[2] * b08 + a[3] * b07) * d, (a[14] * b02 - a[12] * b05 - a[15] * b01) * d, (a[8] * b05 - a[10] * b02 + a[11] * b01) * d,
        (a[4] * b10 - a[5] * b08 + a[7] * b06) * d, (a[1] * b08 - a[0] * b10 - a[3] * b06) * d, (a[12] * b04 - a[13] * b02 + a[15] * b00) * d, (a[9] * b02 - a[8] * b04 - a[11] * b00) * d,
        (a[5] * b07 - a[4] * b09 - a[6] * b06) * d, (a[0] * b09 - a[1] * b07 + a[2] * b06) * d, (a[13] * b01 - a[12] * b03 - a[14] * b00) * d, (a[8] * b03 - a[9] * b01 + a[10] * b00) * d,
    ]
}

fn pt(m: &M, v: &V) -> V {
    [
        m[0] * v[0] + m[4] * v[1] + m[8] * v[2] + m[12],
        m[1] * v[0] + m[5] * v[1] + m[9] * v[2] + m[13],
        m[2] * v[0] + m[6] * v[1] + m[10] * v[2] + m[14],
    ]
}
fn dr(m: &M, v: &V) -> V {
    [m[0] * v[0] + m[4] * v[1] + m[8] * v[2], m[1] * v[0] + m[5] * v[1] + m[9] * v[2], m[2] * v[0] + m[6] * v[1] + m[10] * v[2]]
}
/// `norm(v)` (a zero vector divides by 1).
pub fn norm(v: &V) -> V {
    let l = hypot3(v[0], v[1], v[2]);
    let l = if l == 0.0 || l.is_nan() { 1.0 } else { l };
    [v[0] / l, v[1] / l, v[2] / l]
}
/// viewer (Y up, coordinated) → IFC (Z up, metres)
pub fn to_ifc_point(c: &M, v: &V) -> V {
    let [x, y, z] = pt(&inv4(c), v);
    [x, -z, y]
}
pub fn to_ifc_dir(c: &M, v: &V) -> V {
    let [x, y, z] = dr(&inv4(c), v);
    norm(&[x, -z, y])
}
pub fn from_ifc_point(c: &M, p: &V) -> V {
    pt(c, &[p[0], p[2], -p[1]])
}
pub fn from_ifc_dir(c: &M, d: &V) -> V {
    norm(&dr(c, &[d[0], d[2], -d[1]]))
}

// ---- zip ----

fn crc_table() -> &'static [u32; 256] {
    static T: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0u32; 256];
        for (n, slot) in t.iter_mut().enumerate() {
            let mut c = u32::try_from(n).unwrap_or(0);
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        t
    })
}

/// `crc32(bytes)`
pub fn crc32(b: &[u8]) -> u32 {
    let t = crc_table();
    let mut c = 0xffff_ffffu32;
    for &x in b {
        c = t.get(usize::from(u8::try_from((c ^ u32::from(x)) & 0xff).unwrap_or(0))).copied().unwrap_or(0) ^ (c >> 8);
    }
    c ^ 0xffff_ffff
}

/// `zip(entries)`: stored entries, UTF-8 names (flag 0x0800), DOS date 0x21, as the JS writes them.
pub fn zip(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let u16b = |v: u32| [u8::try_from(v & 255).unwrap_or(0), u8::try_from((v >> 8) & 255).unwrap_or(0)];
    let u32b = |v: u32| v.to_le_bytes();
    let mut parts: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    let mut off: u32 = 0;
    for (name, data) in entries {
        let nb = name.as_bytes();
        let crc = crc32(data);
        let n = u32::try_from(data.len()).unwrap_or(u32::MAX);
        let nl = u32::try_from(nb.len()).unwrap_or(u32::MAX);
        let mut local = Vec::with_capacity(30);
        local.extend_from_slice(&u32b(0x0403_4b50));
        for v in [20, 0x0800, 0, 0, 0x21] {
            local.extend_from_slice(&u16b(v));
        }
        local.extend_from_slice(&u32b(crc));
        local.extend_from_slice(&u32b(n));
        local.extend_from_slice(&u32b(n));
        local.extend_from_slice(&u16b(nl));
        local.extend_from_slice(&u16b(0));
        parts.extend_from_slice(&local);
        parts.extend_from_slice(nb);
        parts.extend_from_slice(data);
        central.extend_from_slice(&u32b(0x0201_4b50));
        for v in [20, 20, 0x0800, 0, 0, 0x21] {
            central.extend_from_slice(&u16b(v));
        }
        central.extend_from_slice(&u32b(crc));
        central.extend_from_slice(&u32b(n));
        central.extend_from_slice(&u32b(n));
        central.extend_from_slice(&u16b(nl));
        for _ in 0..4 {
            central.extend_from_slice(&u16b(0));
        }
        central.extend_from_slice(&u32b(0));
        central.extend_from_slice(&u32b(off));
        central.extend_from_slice(nb);
        let add = u32::try_from(local.len().saturating_add(nb.len()).saturating_add(data.len())).unwrap_or(u32::MAX);
        off = off.wrapping_add(add);
    }
    let count = u32::try_from(entries.len()).unwrap_or(u32::MAX);
    let mut end = Vec::with_capacity(22);
    end.extend_from_slice(&u32b(0x0605_4b50));
    end.extend_from_slice(&u16b(0));
    end.extend_from_slice(&u16b(0));
    end.extend_from_slice(&u16b(count));
    end.extend_from_slice(&u16b(count));
    end.extend_from_slice(&u32b(u32::try_from(central.len()).unwrap_or(u32::MAX)));
    end.extend_from_slice(&u32b(off));
    end.extend_from_slice(&u16b(0));
    let mut out = parts;
    out.extend_from_slice(&central);
    out.extend_from_slice(&end);
    out
}

fn rd16(b: &[u8], p: usize) -> Option<u32> {
    let s = b.get(p..p.checked_add(2)?)?;
    Some(u32::from(u16::from_le_bytes([*s.first()?, *s.get(1)?])))
}
fn rd32(b: &[u8], p: usize) -> Option<u32> {
    let s = b.get(p..p.checked_add(4)?)?;
    Some(u32::from_le_bytes([*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?]))
}

fn usz(v: u32) -> usize {
    usize::try_from(v).unwrap_or(usize::MAX)
}

/// `new TextDecoder().decode(bytes)` as UTF-16 (invalid sequences → U+FFFD, a leading BOM dropped).
pub fn decode(b: &[u8]) -> Vec<u16> {
    let s = String::from_utf8_lossy(b);
    let mut u: Vec<u16> = s.encode_utf16().collect();
    if u.first() == Some(&0xFEFF) {
        u.remove(0);
    }
    u
}

/// `DecompressionStream('deflate-raw')` on the whole input: Err("") for a broken or cut-off stream, the trailing
/// junk message for bytes after its end (Node's texts). Output past `keep` bytes is not kept (only counted).
fn inflate(data: &[u8], keep: u64) -> Result<(Vec<u8>, u64), String> {
    let mut z = Inflate::new(false, 15);
    let mut out: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; 1 << 16];
    let keep = usize::try_from(keep).unwrap_or(usize::MAX);
    loop {
        let pos = usize::try_from(z.total_in()).unwrap_or(usize::MAX);
        let before_out = z.total_out();
        let input = data.get(pos..).unwrap_or(&[]);
        let r = z.decompress(input, &mut buf, InflateFlush::NoFlush);
        let made = usize::try_from(z.total_out().saturating_sub(before_out)).unwrap_or(0);
        if out.len() <= keep {
            let room = keep.saturating_add(1).saturating_sub(out.len());
            out.extend_from_slice(buf.get(..made.min(room)).unwrap_or(&[]));
        }
        if z.total_out() > MAX_INFLATE_WORK {
            return Ok((out, z.total_out()));
        }
        let consumed = usize::try_from(z.total_in()).unwrap_or(usize::MAX).saturating_sub(pos);
        match r {
            Ok(Status::StreamEnd) => {
                if usize::try_from(z.total_in()).unwrap_or(usize::MAX) < data.len() {
                    return Err("Trailing junk found after the end of the compressed stream".into());
                }
                return Ok((out, z.total_out()));
            }
            Ok(_) => {
                if made == 0 && consumed == 0 {
                    return Err(String::new());
                }
            }
            Err(_) => return Err(String::new()),
        }
    }
}

/// `unzip(bytes)` → name (UTF-16) → data, in Map order; Err = the JS's error text.
pub fn unzip(b: &[u8]) -> Result<OMap<Vec<u16>, Vec<u8>>, String> {
    let len = b.len();
    let mut eocd = None;
    if len >= 22 {
        let hi = len.saturating_sub(22);
        let lo = len.saturating_sub(22).saturating_sub(65535);
        let mut i = hi;
        loop {
            if rd32(b, i) == Some(0x0605_4b50) {
                eocd = Some(i);
                break;
            }
            if i == lo {
                break;
            }
            i = i.saturating_sub(1);
        }
    }
    let Some(eocd) = eocd else { return Err("not a zip".into()) };
    let count = rd16(b, eocd.saturating_add(10)).unwrap_or(0);
    let cd_off = rd32(b, eocd.saturating_add(16)).unwrap_or(0);
    if count > u32::from(MAX_ENTRIES) {
        return Err("too many files in the zip".into());
    }
    let mut out: OMap<Vec<u16>, Vec<u8>> = OMap::new();
    let mut p = usz(cd_off);
    let mut total: u64 = 0;
    for _ in 0..count {
        if p.saturating_add(46) > len || rd32(b, p) != Some(0x0201_4b50) {
            return Err("broken zip directory".into());
        }
        let r16 = |o: usize| rd16(b, p.saturating_add(o)).unwrap_or(0);
        let r32 = |o: usize| rd32(b, p.saturating_add(o)).unwrap_or(0);
        let (method, csize, usize_, nlen, xlen, clen, lho) = (r16(10), r32(20), r32(24), r16(28), r16(30), r16(32), r32(42));
        let ns = p.saturating_add(46);
        let name = decode(b.get(ns.min(len)..ns.saturating_add(usz(nlen)).min(len)).unwrap_or(&[]));
        p = ns.saturating_add(usz(nlen)).saturating_add(usz(xlen)).saturating_add(usz(clen));
        if name.last() == Some(&u16::from(b'/')) {
            continue;
        }
        if u64::from(usize_) > MAX_FILE {
            return Err("a file in the zip is too large".into());
        }
        total = total.saturating_add(u64::from(usize_));
        if total > MAX_TOTAL {
            return Err("a file in the zip is too large".into());
        }
        let lho = usz(lho);
        if lho.saturating_add(30) > len || rd32(b, lho) != Some(0x0403_4b50) {
            return Err("broken zip entry".into());
        }
        let start = lho.saturating_add(30).saturating_add(usz(rd16(b, lho.saturating_add(26)).unwrap_or(0))).saturating_add(usz(rd16(
            b,
            lho.saturating_add(28),
        )
        .unwrap_or(0)));
        let raw = b.get(start.min(len)..start.saturating_add(usz(csize)).min(len)).unwrap_or(&[]);
        let data = match method {
            0 => raw.to_vec(),
            8 => {
                let (d, n) = inflate(raw, MAX_FILE)?;
                if n > MAX_FILE {
                    return Err("a file in the zip is too large".into());
                }
                d
            }
            m => return Err(format!("unsupported zip compression {m}")),
        };
        if u64::try_from(data.len()).unwrap_or(u64::MAX) > MAX_FILE {
            return Err("a file in the zip is too large".into());
        }
        let key: Vec<u16> = name.iter().map(|&c| if c == u16::from(b'\\') { u16::from(b'/') } else { c }).collect();
        out.set(key, data);
    }
    Ok(out)
}

// ---- write ----

/// `esc(s)` on `String(s ?? '')`.
fn esc(v: Option<&J>) -> String {
    let s = match v {
        None | Some(J::Null) => String::new(),
        Some(x) => cc_cad::js::to_js_string(x),
    };
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let u = u32::from(c);
        if u <= 0x08 || u == 0x0b || u == 0x0c || (0x0e..=0x1f).contains(&u) || u == 0xFFFE || u == 0xFFFF {
            continue;
        }
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

fn truthy(v: Option<&J>) -> bool {
    cc_cad::js::truthy(v)
}

/// `String(Math.round(v * 1e6) / 1e6)`
fn f6(v: Option<&J>) -> String {
    let x = cc_cad::js::to_number_opt(v);
    num_str(round(x * 1e6) / 1e6)
}

fn p3(tag: &str, v: Option<&J>) -> String {
    let at = |i: usize| v.and_then(|a| a.get(i));
    format!("<{tag}><X>{}</X><Y>{}</Y><Z>{}</Z></{tag}>", f6(at(0)), f6(at(1)), f6(at(2)))
}

fn comps(list: Option<&J>) -> String {
    list.and_then(J::as_array)
        .map(|a| a.iter().map(|g| format!("<Component IfcGuid=\"{}\"/>", esc(Some(g)))).collect::<String>())
        .unwrap_or_default()
}

fn len_truthy(v: Option<&J>) -> bool {
    match v {
        Some(J::Array(a)) => !a.is_empty(),
        Some(J::String(s)) => !s.is_empty(),
        _ => false,
    }
}

/// `viewpointXml(vp)`
pub fn viewpoint_xml(vp: &J) -> String {
    let c = vp.get("camera").filter(|c| truthy(Some(c)));
    let mut parts = vec![
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>".to_string(),
        format!("<VisualizationInfo Guid=\"{}\">", esc(vp.get("guid"))),
        "<Components>".into(),
    ];
    if len_truthy(vp.get("selection")) {
        parts.push(format!("<Selection>{}</Selection>", comps(vp.get("selection"))));
    }
    let vis = vp.get("visibility").filter(|v| truthy(Some(v)));
    let (dv, exc) = match vis {
        Some(v) => (truthy(v.get("defaultVisible")), v.get("exceptions")),
        None => (true, None),
    };
    let ex = if len_truthy(exc) { format!("<Exceptions>{}</Exceptions>", comps(exc)) } else { String::new() };
    parts.push(format!("<Visibility DefaultVisibility=\"{}\">{ex}</Visibility>", if dv { "true" } else { "false" }));
    parts.push("</Components>".into());
    if let Some(c) = c {
        let head = format!(
            "{}{}{}",
            p3("CameraViewPoint", c.get("viewPoint")),
            p3("CameraDirection", c.get("direction")),
            p3("CameraUpVector", c.get("up"))
        );
        if c.get("kind").and_then(J::as_str) == Some("orthogonal") {
            parts.push(format!("<OrthogonalCamera>{head}<ViewToWorldScale>{}</ViewToWorldScale></OrthogonalCamera>", f6(c.get("scale"))));
        } else {
            parts.push(format!("<PerspectiveCamera>{head}<FieldOfView>{}</FieldOfView></PerspectiveCamera>", f6(c.get("fov"))));
        }
    }
    if len_truthy(vp.get("clipping")) {
        let planes: String = vp
            .get("clipping")
            .and_then(J::as_array)
            .map(|a| {
                a.iter()
                    .map(|p| {
                        format!(
                            "<ClippingPlane>{}{}</ClippingPlane>",
                            p3("Location", p.get("location")),
                            p3("Direction", p.get("direction"))
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        parts.push(format!("<ClippingPlanes>{planes}</ClippingPlanes>"));
    }
    parts.push("</VisualizationInfo>".into());
    parts.concat()
}

fn or_bimcraft(v: Option<&J>) -> String {
    if truthy(v) { esc(v) } else { "BimCraft".into() }
}

/// `markupXml(t, file)`
pub fn markup_xml(t: &J, file: Option<&J>) -> String {
    let mut p = vec!["<?xml version=\"1.0\" encoding=\"UTF-8\"?>".to_string(), "<Markup>".into()];
    if let Some(f) = file.filter(|f| truthy(f.get("name"))) {
        let proj = if truthy(f.get("ifcProject")) { format!(" IfcProject=\"{}\"", esc(f.get("ifcProject"))) } else { String::new() };
        let date = if truthy(f.get("date")) { format!("<Date>{}</Date>", esc(f.get("date"))) } else { String::new() };
        p.push(format!("<Header><File{proj} isExternal=\"true\"><Filename>{}</Filename>{date}</File></Header>", esc(f.get("name"))));
    }
    let ty = if truthy(t.get("type")) { format!(" TopicType=\"{}\"", esc(t.get("type"))) } else { String::new() };
    let st = if truthy(t.get("status")) { format!(" TopicStatus=\"{}\"", esc(t.get("status"))) } else { String::new() };
    p.push(format!("<Topic Guid=\"{}\"{ty}{st}>", esc(t.get("guid"))));
    p.push(format!("<Title>{}</Title>", esc(t.get("title"))));
    p.push(format!("<CreationDate>{}</CreationDate>", esc(t.get("created"))));
    p.push(format!("<CreationAuthor>{}</CreationAuthor>", or_bimcraft(t.get("author"))));
    if truthy(t.get("modified")) {
        p.push(format!("<ModifiedDate>{}</ModifiedDate>", esc(t.get("modified"))));
    }
    if truthy(t.get("description")) {
        p.push(format!("<Description>{}</Description>", esc(t.get("description"))));
    }
    p.push("</Topic>".into());
    for c in t.get("comments").and_then(J::as_array).map(Vec::as_slice).unwrap_or(&[]) {
        let vpt = if truthy(c.get("viewpoint")) { format!("<Viewpoint Guid=\"{}\"/>", esc(c.get("viewpoint"))) } else { String::new() };
        p.push(format!(
            "<Comment Guid=\"{}\"><Date>{}</Date><Author>{}</Author><Comment>{}</Comment>{vpt}</Comment>",
            esc(c.get("guid")),
            esc(c.get("date")),
            or_bimcraft(c.get("author")),
            esc(c.get("text"))
        ));
    }
    for (i, vp) in t.get("viewpoints").and_then(J::as_array).map(Vec::as_slice).unwrap_or(&[]).iter().enumerate() {
        let base = if i > 0 { format!("viewpoint-{}", i.saturating_add(1)) } else { "viewpoint".into() };
        let snap = if truthy(vp.get("snapshot")) {
            format!("<Snapshot>{}.png</Snapshot>", if i > 0 { format!("snapshot-{}", i.saturating_add(1)) } else { "snapshot".into() })
        } else {
            String::new()
        };
        p.push(format!("<Viewpoints Guid=\"{}\"><Viewpoint>{base}.bcfv</Viewpoint>{snap}</Viewpoints>", esc(vp.get("guid"))));
    }
    p.push("</Markup>".into());
    p.concat()
}

fn guid_ok(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36 && b.iter().enumerate().all(|(i, c)| if matches!(i, 8 | 13 | 18 | 23) { *c == b'-' } else { c.is_ascii_hexdigit() })
}

/// Snapshot bytes from JSON (an array of numbers, stored as a Uint8Array does).
fn bytes_of(v: &J) -> Vec<u8> {
    v.as_array()
        .map(|a| a.iter().map(|x| u8::try_from(crate::model::u32_of(x.as_f64().unwrap_or(0.0)) & 255).unwrap_or(0)).collect())
        .unwrap_or_default()
}

/// `writeBcf({ topics, file })` → the zip bytes, or the error text.
pub fn write_bcf(topics: &[J], file: Option<&J>) -> Result<Vec<u8>, String> {
    let mut entries = vec![(
        "bcf.version".to_string(),
        b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><Version VersionId=\"2.1\"><DetailedVersion>2.1</DetailedVersion></Version>".to_vec(),
    )];
    for t in topics {
        let g = match t.get("guid") {
            None => "undefined".to_string(),
            Some(x) => cc_cad::js::to_js_string(x),
        };
        if !guid_ok(&g) {
            return Err(format!("topic GUID {g}"));
        }
        entries.push((format!("{g}/markup.bcf"), markup_xml(t, file).into_bytes()));
        for (i, vp) in t.get("viewpoints").and_then(J::as_array).map(Vec::as_slice).unwrap_or(&[]).iter().enumerate() {
            let n = if i > 0 { format!("viewpoint-{}", i.saturating_add(1)) } else { "viewpoint".into() };
            entries.push((format!("{g}/{n}.bcfv"), viewpoint_xml(vp).into_bytes()));
            if let Some(s) = vp.get("snapshot").filter(|s| truthy(Some(s))) {
                let sn = if i > 0 { format!("snapshot-{}", i.saturating_add(1)) } else { "snapshot".into() };
                entries.push((format!("{g}/{sn}.png"), bytes_of(s)));
            }
        }
    }
    Ok(zip(&entries))
}

// ---- read ----

#[derive(Clone, Debug, PartialEq)]
pub struct Camera {
    pub perspective: bool,
    pub view_point: V,
    pub direction: V,
    pub up: V,
    /// fov (perspective) or scale (orthogonal)
    pub size: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Viewpoint {
    pub guid: Vec<u16>,
    pub camera: Option<Camera>,
    pub selection: Vec<Vec<u16>>,
    pub default_visible: bool,
    pub exceptions: Vec<Vec<u16>>,
    pub clipping: Vec<(V, V)>,
    pub snapshot: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Comment {
    pub guid: Vec<u16>,
    pub date: Vec<u16>,
    pub author: Vec<u16>,
    pub text: Vec<u16>,
    pub viewpoint: Option<Vec<u16>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Topic {
    pub guid: Vec<u16>,
    pub title: Vec<u16>,
    pub status: Vec<u16>,
    pub ty: Vec<u16>,
    pub author: Vec<u16>,
    pub created: Vec<u16>,
    pub modified: Option<Vec<u16>>,
    pub description: Vec<u16>,
    pub comments: Vec<Comment>,
    pub viewpoints: Vec<Viewpoint>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Read {
    pub version: Vec<u16>,
    pub topics: Vec<Topic>,
    pub notes: Vec<Vec<u16>>,
}

/// `Number(v)` finite, else None.
fn num(v: &[u16]) -> Option<f64> {
    let x = crate::js::str_num(&lossy(v));
    x.is_finite().then_some(x)
}

fn xyz(d: &Doc, n: Option<usize>) -> Option<V> {
    let n = n?;
    let x = num(&d.txt(Some(n), "X"))?;
    let y = num(&d.txt(Some(n), "Y"))?;
    let z = num(&d.txt(Some(n), "Z"))?;
    Some([x, y, z])
}

fn or_empty(v: Option<&[u16]>) -> Vec<u16> {
    v.filter(|s| !s.is_empty()).map(<[u16]>::to_vec).unwrap_or_default()
}

fn guids(d: &Doc, parent: Option<usize>) -> Vec<Vec<u16>> {
    d.kids(parent, "Component")
        .into_iter()
        .filter_map(|c| d.attr(Some(c), "IfcGuid").filter(|g| !g.is_empty()).map(<[u16]>::to_vec))
        .take(100_000)
        .collect()
}

fn read_viewpoint(d: &Doc, notes: &mut Vec<Vec<u16>>) -> Option<Viewpoint> {
    let vi = d.kid(Some(0), "VisualizationInfo")?;
    let c = d.kid(Some(vi), "Components");
    let mut selection = guids(d, d.kid(c, "Selection"));
    let vis_n = d.kid(c, "Visibility");
    let (default_visible, mut exceptions) = match vis_n {
        Some(v) => (d.attr(Some(v), "DefaultVisibility") != Some(&u16s("false")[..]), guids(d, d.kid(Some(v), "Exceptions"))),
        None => (true, Vec::new()),
    };
    for k in d.kids(c, "Component") {
        let g = d.attr(Some(k), "IfcGuid").filter(|g| !g.is_empty());
        if d.attr(Some(k), "Selected") == Some(&u16s("true")[..])
            && let Some(g) = g
        {
            selection.push(g.to_vec());
        }
        if d.attr(Some(k), "Visible") == Some(&u16s("false")[..])
            && let Some(g) = g
        {
            exceptions.push(g.to_vec());
        }
    }
    let pc = d.kid(Some(vi), "PerspectiveCamera");
    let oc = d.kid(Some(vi), "OrthogonalCamera");
    let mut camera = None;
    if let Some(cam) = pc.or(oc) {
        let vp = xyz(d, d.kid(Some(cam), "CameraViewPoint"));
        let dir = xyz(d, d.kid(Some(cam), "CameraDirection"));
        let up = xyz(d, d.kid(Some(cam), "CameraUpVector"));
        match (vp, dir, up) {
            (Some(vp), Some(dir), Some(up)) => {
                let size = if pc.is_some() { num(&d.txt(pc, "FieldOfView")) } else { num(&d.txt(oc, "ViewToWorldScale")) };
                let dflt = if pc.is_some() { 60.0 } else { 10.0 };
                camera = Some(Camera {
                    perspective: pc.is_some(),
                    view_point: vp,
                    direction: norm(&dir),
                    up: norm(&up),
                    size: size.filter(|s| *s != 0.0).unwrap_or(dflt),
                });
            }
            _ => notes.push(u16s("A viewpoint camera is incomplete and was left out.")),
        }
    }
    let clipping = d
        .kids(d.kid(Some(vi), "ClippingPlanes"), "ClippingPlane")
        .into_iter()
        .filter_map(|p| Some((xyz(d, d.kid(Some(p), "Location"))?, xyz(d, d.kid(Some(p), "Direction"))?)))
        .map(|(l, dd)| (l, norm(&dd)))
        .collect();
    Some(Viewpoint { guid: or_empty(d.attr(Some(vi), "Guid")), camera, selection, default_visible, exceptions, clipping, snapshot: None })
}

fn concat(parts: &[&[u16]]) -> Vec<u16> {
    parts.concat()
}

fn contains(h: &[u16], n: &str) -> bool {
    crate::xml::index_of(h, &u16s(n), 0).is_some()
}

/// `readBcf(bytes)` → the topics, or the error text the JS throws.
pub fn read_bcf(bytes: &[u8]) -> Result<Read, String> {
    let files = unzip(bytes)?;
    let mut notes: Vec<Vec<u16>> = Vec::new();
    let xml = |name: &[u16], notes: &mut Vec<Vec<u16>>| -> Option<Doc> {
        let data = files.get(&name.to_vec()).map(|d| decode(d)).unwrap_or_else(|| u16s("undefined"));
        match parse_xml(&data) {
            Ok(d) => Some(d),
            Err(e) => {
                notes.push(concat(&[name, &u16s(": "), &u16s(&e)]));
                None
            }
        }
    };
    let mut version = Vec::new();
    let vname = u16s("bcf.version");
    if files.has(&vname)
        && let Some(d) = xml(&vname, &mut notes)
    {
        version = or_empty(d.attr(d.kid(Some(0), "Version"), "VersionId"));
    }
    let slash = u16::from(b'/');
    let suffix = u16s("/markup.bcf");
    let mut names: Vec<Vec<u16>> = files
        .keys
        .iter()
        .filter(|n| {
            n.len() > suffix.len()
                && n.ends_with(&suffix)
                && !n.get(..n.len().saturating_sub(suffix.len())).is_some_and(|f| f.contains(&slash))
        })
        .cloned()
        .collect();
    names.sort();
    let mut topics = Vec::new();
    for name in names {
        if topics.len() >= MAX_TOPICS {
            notes.push(u16s(&format!("Only the first {MAX_TOPICS} issues were read.")));
            break;
        }
        let folder: Vec<u16> = name.split(|&c| c == slash).next().unwrap_or(&[]).to_vec();
        let d = xml(&name, &mut notes);
        let Some(d) = d else {
            notes.push(concat(&[&name, &u16s(": no Topic")]));
            continue;
        };
        let m = d.kid(Some(0), "Markup");
        let Some(tp) = d.kid(m, "Topic") else {
            notes.push(concat(&[&name, &u16s(": no Topic")]));
            continue;
        };
        let modified = d.txt(Some(tp), "ModifiedDate");
        let mut t = Topic {
            guid: d.attr(Some(tp), "Guid").filter(|g| !g.is_empty()).map_or_else(|| folder.clone(), <[u16]>::to_vec),
            title: d.txt(Some(tp), "Title"),
            status: or_empty(d.attr(Some(tp), "TopicStatus")),
            ty: or_empty(d.attr(Some(tp), "TopicType")),
            author: d.txt(Some(tp), "CreationAuthor"),
            created: d.txt(Some(tp), "CreationDate"),
            modified: if modified.is_empty() { None } else { Some(modified) },
            description: d.txt(Some(tp), "Description"),
            comments: d
                .kids(m, "Comment")
                .into_iter()
                .map(|c| Comment {
                    guid: or_empty(d.attr(Some(c), "Guid")),
                    date: d.txt(Some(c), "Date"),
                    author: d.txt(Some(c), "Author"),
                    text: d.txt(Some(c), "Comment"),
                    viewpoint: d.attr(d.kid(Some(c), "Viewpoint"), "Guid").filter(|g| !g.is_empty()).map(<[u16]>::to_vec),
                })
                .collect(),
            viewpoints: Vec::new(),
        };
        let mut refs: Vec<(Vec<u16>, Vec<u16>, Vec<u16>)> = d
            .kids(m, "Viewpoints")
            .into_iter()
            .map(|v| (or_empty(d.attr(Some(v), "Guid")), d.txt(Some(v), "Viewpoint"), d.txt(Some(v), "Snapshot")))
            .collect();
        let vpath = concat(&[&folder, &u16s("/viewpoint.bcfv")]);
        if refs.is_empty() && files.has(&vpath) {
            let snap = if files.has(&concat(&[&folder, &u16s("/snapshot.png")])) { u16s("snapshot.png") } else { Vec::new() };
            refs.push((Vec::new(), u16s("viewpoint.bcfv"), snap));
        }
        for (rguid, rfile, rsnap) in refs {
            let path = concat(&[&folder, &[slash], &rfile]);
            if rfile.is_empty() || contains(&rfile, "..") || !files.has(&path) {
                if !rfile.is_empty() {
                    notes.push(concat(&[&path, &u16s(" is missing")]));
                }
                continue;
            }
            let Some(vd) = xml(&path, &mut notes) else { continue };
            let Some(mut vp) = read_viewpoint(&vd, &mut notes) else { continue };
            if vp.guid.is_empty() {
                vp.guid = rguid;
            }
            if !rsnap.is_empty() && !contains(&rsnap, "..") {
                let sp = concat(&[&folder, &[slash], &rsnap]);
                if let Some(dd) = files.get(&sp) {
                    let (a, b) = (dd.first().copied(), dd.get(1).copied());
                    if (a == Some(0x89) && b == Some(0x50)) || (a == Some(0xff) && b == Some(0xd8)) {
                        vp.snapshot = Some(dd.clone());
                    }
                }
            }
            t.viewpoints.push(vp);
        }
        topics.push(t);
    }
    if topics.is_empty() && version.is_empty() {
        return Err("not a BCF file".into());
    }
    Ok(Read { version, topics, notes })
}

// ---- JSON (UTF-16 strings kept exact) ----

fn js(out: &mut String, s: &[u16]) {
    push_json_str16(out, s);
}

fn jnum(out: &mut String, x: f64) {
    out.push_str(&crate::js::jn(x).to_string());
}

fn jv3(out: &mut String, v: &V) {
    out.push('[');
    for (i, x) in v.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        jnum(out, *x);
    }
    out.push(']');
}

fn jlist(out: &mut String, l: &[Vec<u16>]) {
    out.push('[');
    for (i, s) in l.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        js(out, s);
    }
    out.push(']');
}

impl Read {
    /// The read result as JSON (snapshots as digests).
    pub fn json(&self) -> String {
        let mut o = String::from("{\"version\":");
        js(&mut o, &self.version);
        o.push_str(",\"notes\":");
        jlist(&mut o, &self.notes);
        o.push_str(",\"topics\":[");
        for (ti, t) in self.topics.iter().enumerate() {
            if ti > 0 {
                o.push(',');
            }
            o.push_str("{\"guid\":");
            js(&mut o, &t.guid);
            for (k, v) in [
                ("title", &t.title),
                ("status", &t.status),
                ("type", &t.ty),
                ("author", &t.author),
                ("created", &t.created),
                ("description", &t.description),
            ] {
                o.push_str(&format!(",\"{k}\":"));
                js(&mut o, v);
            }
            o.push_str(",\"modified\":");
            match &t.modified {
                Some(m) => js(&mut o, m),
                None => o.push_str("null"),
            }
            o.push_str(",\"comments\":[");
            for (ci, c) in t.comments.iter().enumerate() {
                if ci > 0 {
                    o.push(',');
                }
                o.push_str("{\"guid\":");
                js(&mut o, &c.guid);
                o.push_str(",\"date\":");
                js(&mut o, &c.date);
                o.push_str(",\"author\":");
                js(&mut o, &c.author);
                o.push_str(",\"text\":");
                js(&mut o, &c.text);
                o.push_str(",\"viewpoint\":");
                match &c.viewpoint {
                    Some(v) => js(&mut o, v),
                    None => o.push_str("null"),
                }
                o.push('}');
            }
            o.push_str("],\"viewpoints\":[");
            for (vi, v) in t.viewpoints.iter().enumerate() {
                if vi > 0 {
                    o.push(',');
                }
                o.push_str("{\"guid\":");
                js(&mut o, &v.guid);
                o.push_str(",\"camera\":");
                match &v.camera {
                    None => o.push_str("null"),
                    Some(c) => {
                        o.push_str(if c.perspective { "{\"kind\":\"perspective\"" } else { "{\"kind\":\"orthogonal\"" });
                        o.push_str(",\"viewPoint\":");
                        jv3(&mut o, &c.view_point);
                        o.push_str(",\"direction\":");
                        jv3(&mut o, &c.direction);
                        o.push_str(",\"up\":");
                        jv3(&mut o, &c.up);
                        o.push_str(if c.perspective { ",\"fov\":" } else { ",\"scale\":" });
                        jnum(&mut o, c.size);
                        o.push('}');
                    }
                }
                o.push_str(",\"selection\":");
                jlist(&mut o, &v.selection);
                o.push_str(",\"visibility\":{\"defaultVisible\":");
                o.push_str(if v.default_visible { "true" } else { "false" });
                o.push_str(",\"exceptions\":");
                jlist(&mut o, &v.exceptions);
                o.push_str("},\"clipping\":[");
                for (pi, (l, d)) in v.clipping.iter().enumerate() {
                    if pi > 0 {
                        o.push(',');
                    }
                    o.push_str("{\"location\":");
                    jv3(&mut o, l);
                    o.push_str(",\"direction\":");
                    jv3(&mut o, d);
                    o.push('}');
                }
                o.push_str("],\"snapshot\":");
                match &v.snapshot {
                    Some(b) => o.push_str(&crate::digest::bytes(b).to_string()),
                    None => o.push_str("null"),
                }
                o.push('}');
            }
            o.push_str("]}");
        }
        o.push_str("]}");
        o
    }
}

/// `parseXml` result as JSON (the tree with attributes in assignment order) or `{ "$error": text }`.
pub fn xml_json(text: &[u16]) -> String {
    match parse_xml(text) {
        Err(e) => {
            let mut o = String::from("{\"$error\":");
            js(&mut o, &u16s(&e));
            o.push('}');
            o
        }
        Ok(d) => {
            fn node(d: &Doc, i: usize, o: &mut String, depth: usize) {
                let Some(n) = d.node(i) else {
                    o.push_str("null");
                    return;
                };
                o.push_str("{\"name\":");
                js(o, &n.name);
                o.push_str(",\"attrs\":{");
                // object semantics: a repeated name keeps its first place and its last value
                let mut seen: Vec<&[u16]> = Vec::new();
                let mut first = true;
                for (k, _) in &n.attrs {
                    if seen.contains(&k.as_slice()) {
                        continue;
                    }
                    seen.push(k);
                    if !first {
                        o.push(',');
                    }
                    first = false;
                    js(o, k);
                    o.push(':');
                    let v = n.attrs.iter().rev().find(|(kk, _)| kk == k).map_or(&[][..], |(_, v)| v.as_slice());
                    js(o, v);
                }
                o.push_str("},\"text\":");
                js(o, &n.text);
                o.push_str(",\"children\":[");
                if depth < 80 {
                    for (ci, c) in n.children.iter().enumerate() {
                        if ci > 0 {
                            o.push(',');
                        }
                        node(d, *c, o, depth.saturating_add(1));
                    }
                }
                o.push_str("]}");
            }
            let mut o = String::new();
            node(&d, 0, &mut o, 0);
            o
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cameras() {
        let c: M = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -100.0, -2.0, 50.0, 1.0];
        let ip = to_ifc_point(&c, &[3.0, 4.0, 5.0]);
        assert_eq!(ip, [103.0, 45.0, 6.0]);
        assert_eq!(from_ifc_point(&c, &ip), [3.0, 4.0, 5.0]);
    }

    #[test]
    fn write_then_read() {
        let topics: Vec<J> = serde_json::from_str(r#"[{"guid":"0f8fad5b-d9cb-469f-a165-70867728950e","title":"A <b> & \"c\"","status":"Open","type":"Issue","author":"x","created":"2026-10-12T10:00:00Z","description":"Line 1\nLine 2","comments":[],"viewpoints":[{"guid":"7c9e6679-7425-40de-944b-e07fc1f90ae7","camera":{"kind":"perspective","viewPoint":[10,-5,3],"direction":[0,1,0],"up":[0,0,1],"fov":45},"selection":["g1"],"visibility":{"defaultVisible":true,"exceptions":["g2"]},"clipping":[{"location":[0,0,1.5],"direction":[0,0,1]}],"snapshot":[137,80,78,71]}]}]"#).unwrap();
        let z = write_bcf(&topics, None).unwrap();
        let r = read_bcf(&z).unwrap();
        assert_eq!(lossy(&r.version), "2.1");
        assert_eq!(r.topics.len(), 1);
        assert_eq!(lossy(&r.topics[0].title), "A <b> & \"c\"");
        let v = &r.topics[0].viewpoints[0];
        assert_eq!(v.camera.as_ref().unwrap().size, 45.0);
        assert_eq!(v.snapshot.as_deref(), Some(&[137u8, 80, 78, 71][..]));
        assert_eq!(read_bcf(&[1, 2, 3]).unwrap_err(), "not a zip");
    }
}
