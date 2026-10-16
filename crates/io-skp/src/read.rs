//! cc-skp's front door (port of `lib/skp/read.js`): reads the header records, picks the container, adds counts.
//!
//! Every file starts with UTF-16LE string records, each "FF FE FF" + a length + that many UTF-16 code units
//! (length: u8; 0xFF → u16 follows; 0xFFFF → u32 follows): record 1 "SketchUp Model", record 2 the version in braces
//! ("{25.0.575}"). Then SketchUp 2021+ (VFF) writes "VFF" and a ZIP archive; older files a 16-byte id and the MFC
//! object stream.

use crate::model::{Container, Counts, Skp};
use crate::util::{LIMITS, Limits, Res, SkpError, damaged, err, round_pos};
use crate::{legacy, vff};

const MAGIC: &str = "SketchUp Model";

/// The header handed to the container readers: `{ version, major, bodyStart }`.
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    /// Record 2 without the braces.
    pub version: String,
    /// `parseInt` of the first number (a JS number).
    pub major: f64,
    /// Byte offset just past record 2.
    pub body_start: usize,
}

/// `sniffSkp(bytes)` → `{ version, major, bodyStart, container }`.
#[derive(Debug, Clone, PartialEq)]
pub struct Sniff {
    pub header: Header,
    pub container: Container,
}

/// One "FF FE FF" string record at p → (UTF-16 code units, end), or None.
fn string_record(b: &[u8], p: usize) -> Option<(Vec<u16>, usize)> {
    if p.checked_add(4)? > b.len() || b.get(p..p.checked_add(3)?)? != [0xff, 0xfe, 0xff] {
        return None;
    }
    let mut n = usize::from(*b.get(p.checked_add(3)?)?);
    let mut q = p.checked_add(4)?;
    if n == 0xff {
        if q.checked_add(2)? > b.len() {
            return None;
        }
        n = usize::from(u16::from_le_bytes([*b.get(q)?, *b.get(q.checked_add(1)?)?]));
        q = q.checked_add(2)?;
        if n == 0xffff {
            if q.checked_add(4)? > b.len() {
                return None;
            }
            let v = u32::from_le_bytes([*b.get(q)?, *b.get(q.checked_add(1)?)?, *b.get(q.checked_add(2)?)?, *b.get(q.checked_add(3)?)?]);
            n = usize::try_from(v).ok()?;
            q = q.checked_add(4)?;
        }
    }
    let end = q.checked_add(n.checked_mul(2)?)?;
    if n > 4096 || end > b.len() {
        return None;
    }
    let units = b.get(q..end)?.as_chunks::<2>().0.iter().map(|&c| u16::from_le_bytes(c)).collect();
    Some((units, end))
}

/// `/^\{(\d+)\.([\d.]*)\}$/` on the code units → (major digits, rest).
fn version_match(u: &[u16]) -> Option<(String, String)> {
    let inner = u.strip_prefix(&[u16::from(b'{')])?.strip_suffix(&[u16::from(b'}')])?;
    let is_digit = |c: &u16| (0x30..=0x39).contains(c);
    let n = inner.iter().take_while(|c| is_digit(c)).count();
    if n == 0 || inner.get(n) != Some(&u16::from(b'.')) {
        return None;
    }
    let rest = inner.get(n.checked_add(1)?..)?;
    if !rest.iter().all(|c| is_digit(c) || *c == u16::from(b'.')) {
        return None;
    }
    let text = |s: &[u16]| s.iter().map(|&c| char::from(u8::try_from(c).unwrap_or(b'?'))).collect::<String>();
    Some((text(inner.get(..n)?), text(rest)))
}

/// Read the header records → `{ version, major, bodyStart, container }`.
pub fn sniff_skp(b: &[u8]) -> Res<Sniff> {
    let not_skp = || err("not_skp", "ccskp.not_skp", &[]);
    let r1 = string_record(b, 0);
    let magic: Vec<u16> = MAGIC.encode_utf16().collect();
    let Some((t1, end1)) = r1.as_ref().filter(|(t, _)| *t == magic).cloned() else {
        // The start of "SketchUp Model" but the file ends inside it: a cut-off SketchUp file, not some other file.
        let mut head: Vec<u8> = vec![0xff, 0xfe, 0xff, 14];
        for c in MAGIC.bytes() {
            head.push(c);
            head.push(0);
        }
        if r1.is_none() && b.len() >= 8 && b.len() < head.len() && head.starts_with(b) {
            return Err(SkpError::new("damaged", crate::messages::damaged_text("ccskp.damaged.header_cut", &[])));
        }
        return Err(not_skp());
    };
    let _ = t1;
    let r2 = string_record(b, end1);
    let Some((major_digits, rest, body_start)) = r2.and_then(|(u, end)| version_match(&u).map(|(m, r)| (m, r, end))) else {
        return Err(damaged("ccskp.damaged.no_version"));
    };
    let version = if rest.is_empty() { major_digits.clone() } else { format!("{major_digits}.{rest}") };
    let major: f64 = major_digits.parse().unwrap_or(f64::INFINITY);
    // VFF: "VFF" right after the version, or a ZIP local header within the next few dozen bytes.
    let mut container = Container::Legacy;
    if b.get(body_start..body_start.saturating_add(3)) == Some(b"VFF") {
        container = Container::Vff;
    } else if let Some(last) = b.len().checked_sub(4) {
        let end = last.min(body_start.saturating_add(64));
        let mut i = body_start;
        while i <= end {
            if b.get(i..i.saturating_add(4)) == Some(&[0x50, 0x4b, 3, 4]) {
                container = Container::Vff;
                break;
            }
            i = i.saturating_add(1);
        }
    }
    Ok(Sniff { header: Header { version, major, body_start }, container })
}

/// Read a .skp file (`readSkp(bytes)`).
pub fn read_skp(bytes: &[u8]) -> Res<Skp> {
    read_skp_with(bytes, &LIMITS)
}

/// `read_skp` with other limits (tests).
pub fn read_skp_with(bytes: &[u8], limits: &Limits) -> Res<Skp> {
    if bytes.len() as u64 > limits.file_bytes {
        let size = format!("{} MB", round_pos(limits.file_bytes as f64 / 1_048_576.0));
        return Err(err("too_big", "ccskp.too_big.file", &[("size", &size)]));
    }
    let sniff = sniff_skp(bytes)?;
    let body = match sniff.container {
        Container::Vff => vff::read_vff(bytes, &sniff.header, limits)?,
        Container::Legacy => legacy::read_legacy(bytes, &sniff.header, limits)?,
    };
    let mut counts = Counts { defs: body.defs.len(), materials: body.materials.len(), layers: body.layers.len(), ..Counts::default() };
    for d in &body.defs {
        counts.faces = counts.faces.saturating_add(d.faces.len());
        counts.edges = counts.edges.saturating_add(d.edges.len());
        counts.insts = counts.insts.saturating_add(d.insts.len());
    }
    Ok(Skp {
        container: sniff.container,
        version: sniff.header.version,
        major: sniff.header.major,
        materials: body.materials,
        layers: body.layers,
        defs: body.defs,
        root: body.root,
        thumbnail: body.thumbnail,
        units: body.units,
        notes: body.notes,
        counts,
    })
}

/// What a container reader returns (the model without format/container/version/major/counts).
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub materials: Vec<crate::model::Material>,
    pub layers: Vec<crate::model::Layer>,
    pub defs: Vec<crate::model::Def>,
    pub root: usize,
    pub thumbnail: Option<crate::model::Image>,
    pub units: Option<&'static str>,
    pub notes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_pattern() {
        let u = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
        assert_eq!(version_match(&u("{25.0.575}")), Some(("25".into(), "0.575".into())));
        assert_eq!(version_match(&u("{17.}")), Some(("17".into(), "".into())));
        assert_eq!(version_match(&u("{17}")), None);
        assert_eq!(version_match(&u("25.0.1")), None);
    }
}
