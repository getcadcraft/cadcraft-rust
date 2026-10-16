//! The R2013+ data storage section AcDb:AcDsPrototype_1b, read side (port of `readAcds` / `readBlob` of
//! lib/dwg/acds.js). Written from the ODA "Open Design Specification for .dwg files" v5.4.1, ch. 24 (file header 24.1,
//! file segments 24.2, segment index 24.2.2.1, data index 24.2.2.2, data segment + blob references 24.2.2.3, blob01
//! 24.2.2.4). The section holds one record per ACIS entity: the entity's handle and its SAB stream. Facts found by
//! experiment are marked "Spec gap:".
//!
//! Positions are kept as f64 like the JS numbers; every read is bounds-checked and fails with the text V8 gives
//! (`readAcds` turns any failure into an "only in part" note, as the JS catch does).

use std::collections::HashMap;

use crate::messages::msg;
use crate::util::{to_i64, to_usize, usize_to_f64};

const SIG: u32 = 0x6472_616A; // "jard" (24.1 file signature)
const SEG_SIG: u32 = 0xD5AC; // 24.2.1
const BLOB_REF: u32 = 0xBB10_6BB1; // 24.2.2.3: dataSize of a record that is a blob reference

/// V8's RangeError text for a DataView read outside the buffer.
const DV_RANGE: &str = "Offset is outside the bounds of the DataView";

type R<T> = Result<T, String>;

/// The records by handle (JS `Map(handle Number → Uint8Array)`; keys as integers) and the notes.
#[derive(Debug, Default)]
pub struct Acds {
    pub records: HashMap<i64, Vec<u8>>,
    pub notes: Vec<String>,
}

struct Dv<'a>(&'a [u8]);
impl Dv<'_> {
    fn at<const N: usize>(&self, p: f64) -> R<[u8; N]> {
        if p.is_nan() || p < 0.0 || p + usize_to_f64(N) > usize_to_f64(self.0.len()) {
            return Err(DV_RANGE.into());
        }
        let i = to_usize(p);
        let s = self.0.get(i..i.saturating_add(N)).ok_or_else(|| DV_RANGE.to_string())?;
        let mut a = [0u8; N];
        a.copy_from_slice(s);
        Ok(a)
    }
    fn u16(&self, p: f64) -> R<u32> {
        Ok(u32::from(u16::from_le_bytes(self.at::<2>(p)?)))
    }
    fn u32(&self, p: f64) -> R<f64> {
        Ok(f64::from(u32::from_le_bytes(self.at::<4>(p)?)))
    }
    fn i32(&self, p: f64) -> R<f64> {
        Ok(f64::from(i32::from_le_bytes(self.at::<4>(p)?)))
    }
    /// `Number(getBigUint64(p))`
    #[allow(clippy::cast_precision_loss)]
    fn u64(&self, p: f64) -> R<f64> {
        Ok(u64::from_le_bytes(self.at::<8>(p)?) as f64)
    }
    fn len(&self) -> f64 {
        usize_to_f64(self.0.len())
    }
    /// `sec.slice(a, b)` / `subarray` for non-negative a (both clamp to the length).
    fn slice(&self, a: f64, b: f64) -> &[u8] {
        let n = self.0.len();
        let cl = |x: f64| if x < 0.0 { n.saturating_sub(to_usize(-x)) } else { to_usize(x).min(n) };
        let (a, b) = (cl(a), cl(b));
        if b <= a { &[] } else { self.0.get(a..b).unwrap_or(&[]) }
    }
}

struct Seg {
    off: f64,
    name: String,
    obj_align: f64,
    data: f64,
}
#[derive(Clone, Copy)]
struct SegEntry {
    offset: f64,
    size: f64,
}

fn js_num(n: f64) -> String {
    let mut s = String::new();
    crate::value::js_num(n, &mut s);
    s
}

fn seg_at(d: &Dv, off: f64) -> R<Seg> {
    if off < 0.0 || off + 48.0 > d.len() || d.u16(off)? != SEG_SIG {
        return Err(msg("ccdwg.err.acds_no_segment", &[("offset", &js_num(off))]));
    }
    let name: String = d.slice(off + 2.0, off + 8.0).iter().map(|&c| char::from(c)).collect();
    Ok(Seg { off, name, obj_align: d.i32(off + 36.0)?, data: off + 48.0 })
}

fn seg(d: &Dv, segs: &[SegEntry], i: f64) -> R<Seg> {
    let s = if i >= 0.0 { segs.get(to_usize(i)) } else { None };
    match s {
        Some(s) if s.offset != 0.0 => seg_at(d, s.offset),
        _ => Err(msg("ccdwg.err.acds_segment", &[("n", &js_num(i))])),
    }
}

/// Read the data storage section. Never fails: a damaged section gives fewer records and a note.
pub fn read_acds(sec: &[u8]) -> Acds {
    let mut out = Acds::default();
    if sec.len() < 0x40 {
        out.notes.push(msg("ccdwg.read.acds_empty", &[]));
        return out;
    }
    let d = Dv(sec);
    if d.u32(0.0).ok() != Some(f64::from(SIG)) {
        out.notes.push(msg("ccdwg.read.acds_no_sig", &[]));
        return out;
    }
    if let Err(e) = read_inner(&d, &mut out) {
        out.notes.push(msg("ccdwg.read.acds_partial", &[("reason", &e)]));
    }
    out
}

fn read_inner(d: &Dv, out: &mut Acds) -> R<()> {
    let seg_idx_offset = d.i32(24.0)?;
    let seg_count = d.i32(32.0)?;
    let dat_idx = d.i32(40.0)?;
    // 24.2.2.1 segment index: the stream offset and size of every file segment, by segment index
    let si = seg_at(d, seg_idx_offset)?;
    if si.name != "segidx" {
        return Err(msg("ccdwg.err.acds_index", &[("name", &si.name)]));
    }
    let mut segs: Vec<SegEntry> = Vec::new();
    let mut i = 0.0;
    let mut p = si.data;
    while i < seg_count && p + 12.0 <= d.len() {
        segs.push(SegEntry { offset: d.u64(p)?, size: d.u32(p + 8.0)? });
        i += 1.0;
        p += 12.0;
    }
    // 24.2.2.2 data index: (segment, local offset, schema) per entry
    let di = seg(d, &segs, dat_idx)?;
    let n = d.i32(di.data)?;
    // (segment, local offset) of the entries, grouped by segment in first-seen order (a JS Map)
    let mut by_seg: Vec<(f64, Vec<f64>)> = Vec::new();
    let mut i = 0.0;
    let mut p = di.data + 8.0;
    while i < n && p + 12.0 <= d.len() {
        let es = d.u32(p)?;
        let local = d.u32(p + 4.0)?;
        if es != 0.0 {
            match by_seg.iter_mut().find(|(s, _)| *s == es) {
                Some((_, l)) => l.push(local),
                None => by_seg.push((es, vec![local])),
            }
        }
        i += 1.0;
        p += 12.0;
    }
    // 24.2.2.3 data segments: record headers at the entries' local offsets, data after the "data start marker"
    for (si2, list) in &by_seg {
        let s = seg(d, &segs, *si2)?;
        if s.name != "_data_" {
            out.notes.push(msg("ccdwg.read.acds_entry", &[("name", &s.name)]));
            continue;
        }
        let seg_size = segs.get(to_usize(*si2)).map_or(f64::NAN, |e| e.size);
        let seg_end = s.off + seg_size;
        // Spec gap: the data index's local offsets count from the end of the segment header (stream position + 0x30),
        // and the "data start marker" is the segment start + (object data alignment offset << 4) — found by experiment.
        let mut heads: Vec<(f64, f64)> = Vec::with_capacity(list.len()); // (handle, local)
        for &e_local in list {
            let p = s.data + e_local;
            d.u32(p)?;
            d.u32(p + 4.0)?;
            let handle = d.u64(p + 8.0)?;
            let local = d.u32(p + 16.0)?;
            heads.push((handle, local));
        }
        let start = s.off + f64::from(crate::util::to_i32(s.obj_align).wrapping_shl(4));
        let mut starts: Vec<f64> = heads.iter().map(|h| start + h.1).collect();
        starts.sort_by(f64::total_cmp);
        for &(handle, local) in &heads {
            let at = start + local;
            let next_at = starts.iter().copied().find(|&x| x > at).unwrap_or(seg_end);
            let max = next_at - at;
            let size = d.u32(at)?;
            if size == f64::from(BLOB_REF) {
                let b = read_blob(d, at + 4.0, &segs)?;
                out.records.insert(to_i64(handle), b);
            } else if size + 4.0 <= max {
                out.records.insert(to_i64(handle), d.slice(at + 4.0, at + 4.0 + size).to_vec());
            } else {
                out.notes.push(msg("ccdwg.read.acds_record_large", &[("handle", &crate::util::hex_of(handle))]));
            }
        }
    }
    Ok(())
}

/// 24.2.2.3.1 a blob reference: the pages live in blob01 segments (24.2.2.4).
fn read_blob(d: &Dv, p: f64, segs: &[SegEntry]) -> R<Vec<u8>> {
    let total = d.u64(p)?;
    let pages = d.u32(p + 8.0)?;
    // Deliberate deviation (safety): the JS allocates `total` bytes whatever it is; every byte of a real blob comes from
    // this section, so a larger total is a damaged file and fails here instead of allocating it.
    if total > d.len() {
        return Err(format!("Invalid typed array length: {}", js_num(total)));
    }
    let mut out = vec![0u8; to_usize(total)];
    let mut q = p + 32.0; // total UInt64, page count, record size, page size, last page size, 2 × unknown
    let mut k = 0.0;
    while k < pages {
        let si = d.u32(q)?;
        let s = seg(d, segs, si)?;
        let b = s.data;
        let start = d.u64(b + 8.0)?;
        let size = d.u64(b + 24.0)?;
        if start > total {
            return Err("offset is out of bounds".into());
        }
        let src = d.slice(b + 32.0, b + 32.0 + size.min(total - start));
        let st = to_usize(start);
        if let Some(dst) = out.get_mut(st..st.saturating_add(src.len())) {
            dst.copy_from_slice(src);
        }
        k += 1.0;
        q += 8.0;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damaged_sections_give_notes() {
        assert_eq!(read_acds(&[]).notes.len(), 1);
        assert_eq!(read_acds(&[0u8; 0x40]).notes.len(), 1);
        let mut s = vec![0u8; 0x40];
        s[..4].copy_from_slice(&SIG.to_le_bytes());
        s[24..28].copy_from_slice(&0x7FFF_FFF0u32.to_le_bytes());
        let a = read_acds(&s);
        assert!(a.records.is_empty());
        assert_eq!(a.notes.len(), 1);
    }
}
