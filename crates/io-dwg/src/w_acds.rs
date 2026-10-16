//! The R2013+ data storage section AcDb:AcDsPrototype_1b, writing side (port of lib/dwg/acds.js writeAcds; ODA spec
//! ch. 24: file header 24.1, segments 24.2, segment index 24.2.2.1, data index 24.2.2.2, data segment + blob references
//! 24.2.2.3, blob01 24.2.2.4, schema index/data 24.2.2.5–6, search 24.2.2.7). One record per ACIS entity: the
//! entity's handle and its SAB stream. Spec gap (see acds.js): records over 0x40000 bytes go to blob01 pages of
//! 0xFFFB0 bytes, the last page taking the rest.

const SIG: u32 = 0x6472_616A; // "jard"
const SEG_SIG: u16 = 0xD5AC;
const BLOB_REF: u32 = 0xBB10_6BB1;
const PAD: u8 = 0x70;
const BLOB_MAX: usize = 0x40000;
const PAGE: usize = 0xFFFB0;
const SCHEMA_NAMES: [&str; 6] = [
    "AcDb_Thumbnail_Schema",
    "AcDb3DSolid_ASM_Data",
    "AcDbDs::TreatedAsObjectDataSchema",
    "AcDbDs::LegacySchema",
    "AcDbDs::IndexedPropertySchema",
    "AcDbDs::HandleAttributeSchema",
];
const PROP_NAMES: [&str; 7] = [
    "AcDbDs::ID",
    "Thumbnail_Data",
    "ASM_Data",
    "AcDbDs::TreatedAsObjectData",
    "AcDbDs::Legacy",
    "AcDs:Indexable",
    "AcDbDs::HandleAttribute",
];

struct Bytes {
    b: Vec<u8>,
}
impl Bytes {
    fn new() -> Self {
        Bytes { b: Vec::new() }
    }
    fn p(&self) -> usize {
        self.b.len()
    }
    fn u8(&mut self, v: u8) {
        self.b.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    fn u32s(&mut self, v: usize) {
        self.u32(crate::util::u32_of_usize(v));
    }
    fn u64(&mut self, v: u64) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    fn u64s(&mut self, v: usize) {
        self.u64(crate::util::usize_to_u64(v));
    }
    fn raw(&mut self, a: &[u8]) {
        self.b.extend_from_slice(a);
    }
    fn str0(&mut self, s: &str) {
        for c in s.bytes() {
            self.u8(c);
        }
        self.u8(0);
    }
    fn pad(&mut self, mult: usize, byte: u8) {
        while !self.p().is_multiple_of(mult) {
            self.u8(byte);
        }
    }
}

/// A file segment: 0x30 header + data, padded to 0x40 with 0x70.
fn segment(name: &str, index: usize, data: &[u8], unknown1: u32, sys_align: usize, obj_align: usize, size_field: Option<u32>) -> Vec<u8> {
    let size = (0x30usize.saturating_add(data.len()).saturating_add(0x3F)) & !0x3F;
    let mut o = Bytes::new();
    o.u16(SEG_SIG);
    let nb = name.as_bytes();
    for i in 0..6 {
        o.u8(nb.get(i).copied().unwrap_or(0));
    }
    o.u32s(index);
    o.u32(unknown1);
    o.u32(size_field.unwrap_or_else(|| crate::util::u32_of_usize(size)));
    o.u32(0);
    o.u32(1);
    o.u32(0);
    o.u32s(sys_align);
    o.u32s(obj_align);
    for _ in 0..8 {
        o.u8(0x55);
    }
    o.raw(data);
    o.pad(0x40, PAD);
    o.b
}

/// 24.2.2.6 schema data → (data, offsets, namesAt)
fn schema_data() -> (Vec<u8>, Vec<usize>, usize) {
    let mut o = Bytes::new();
    for _ in 0..2 {
        for f in [1u32, 1, 1, 0] {
            o.u32(8);
            o.u32(f);
        }
    }
    let mut offsets = Vec::new();
    let prop = |o: &mut Bytes, flags: u32, name_idx: u32, ty: u32, values: &[u64], extra: Option<u32>| {
        o.u32(flags);
        o.u32(name_idx);
        if flags & 2 == 0 {
            o.u32(ty);
        }
        if flags == 1 || flags == 8 {
            o.u32(extra.unwrap_or(0));
        }
        o.u16(u16::try_from(values.len()).unwrap_or(0));
        for &v in values {
            if ty == 10 {
                o.u64(v);
            } else if ty == 7 {
                o.u8(crate::util::u8_of(crate::util::low_u32(v)));
            }
        }
    };
    for (idx, ids, data_name) in [([4u64, 5], [6u64, 7], 1u32), ([0, 1], [2, 3], 2)] {
        offsets.push(o.p());
        o.u16(2);
        for v in idx {
            o.u64(v);
        }
        o.u16(2);
        prop(&mut o, 0, 0, 10, &ids, None);
        prop(&mut o, 0, data_name, 15, &[], None);
    }
    for n in [3u32, 4, 5] {
        offsets.push(o.p());
        o.u16(0);
        o.u16(1);
        prop(&mut o, 0, n, 1, &[], None);
    }
    offsets.push(o.p());
    o.u16(0);
    o.u16(1);
    prop(&mut o, 8, 6, 7, &[0], Some(1));
    o.pad(16, 0x73);
    let names_at = o.p();
    o.u32s(PROP_NAMES.len());
    for n in PROP_NAMES {
        o.str0(n);
    }
    (o.b, offsets, names_at)
}
/// 24.2.2.5 schema index → (data, namesAt)
fn schema_index(dat_seg: usize, offsets: &[usize]) -> (Vec<u8>, usize) {
    let mut o = Bytes::new();
    o.u32s(offsets.len());
    o.u32(0);
    for (i, &off) in offsets.iter().enumerate() {
        o.u32s(i);
        o.u32s(dat_seg);
        o.u32s(off);
    }
    o.u64(0x0AF10C);
    o.u32(8);
    o.u32(0);
    for k in 0..8u32 {
        o.u32s(dat_seg);
        o.u32(k.saturating_mul(8));
        o.u32(2u32.saturating_add(k & 3));
    }
    o.pad(16, PAD);
    let names_at = o.p();
    o.u32s(SCHEMA_NAMES.len());
    for n in SCHEMA_NAMES {
        o.str0(n);
    }
    (o.b, names_at)
}

/// The data storage section for the ACIS records [(handle, SAB)], or None when there are none.
pub fn write_acds(records: &[(f64, Vec<u8>)]) -> Option<Vec<u8>> {
    if records.is_empty() {
        return None;
    }
    let mut recs: Vec<&(f64, Vec<u8>)> = records.iter().collect();
    recs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut segs: Vec<(usize, Vec<u8>)> = Vec::new();
    let mut next_index = 2usize;
    // blob pages first
    let mut blob_refs: Vec<Option<Vec<(usize, usize)>>> = vec![None; recs.len()];
    for (ri, r) in recs.iter().enumerate() {
        let len = r.1.len();
        if len <= BLOB_MAX {
            continue;
        }
        let n = (len / PAGE).max(1);
        let mut pages = Vec::new();
        for k in 0..n {
            let start = k.saturating_mul(PAGE);
            let size = if k.saturating_add(1) == n { len.saturating_sub(start) } else { PAGE };
            let mut h = Bytes::new();
            h.u64s(len);
            h.u64s(start);
            h.u32s(k);
            h.u32s(n);
            h.u64s(size);
            h.raw(r.1.get(start..start.saturating_add(size)).unwrap_or(&[]));
            let index = next_index;
            next_index = next_index.saturating_add(1);
            segs.push((index, segment("blob01", index, &h.b, 1, 0, k, Some(0x30))));
            pages.push((index, size));
        }
        if let Some(slot) = blob_refs.get_mut(ri) {
            *slot = Some(pages);
        }
    }
    // the data segment: record headers, padding to 16 (0x62), then the records
    let data_index = next_index;
    next_index = next_index.saturating_add(1);
    let mut head = Bytes::new();
    let mut body = Bytes::new();
    for (ri, r) in recs.iter().enumerate() {
        head.u32(20);
        head.u32(1);
        head.u64(crate::util::f64_to_u64_wrapping(r.0));
        head.u32s(body.p());
        match blob_refs.get(ri).and_then(Option::as_ref) {
            Some(pages) => {
                body.u32(BLOB_REF);
                body.u64s(r.1.len());
                body.u32s(pages.len());
                body.u32s(32usize.saturating_add(pages.len().saturating_mul(8)));
                body.u32s(PAGE);
                body.u32s(pages.last().map_or(0, |p| p.1));
                body.u32(0);
                body.u32(0);
                for &(i, s) in pages {
                    body.u32s(i);
                    body.u32s(s);
                }
            }
            None => {
                body.u32s(r.1.len());
                body.raw(&r.1);
            }
        }
    }
    let hb = head.b;
    let data_start = (0x30usize.saturating_add(hb.len()).saturating_add(15)) & !15;
    let mut dd = Bytes::new();
    dd.raw(&hb);
    while dd.p() < data_start.saturating_sub(0x30) {
        dd.u8(0x62);
    }
    dd.raw(&body.b);
    segs.push((data_index, segment("_data_", data_index, &dd.b, 0, 0, data_start >> 4, None)));
    // an empty data segment (the converter always adds one)
    let empty_index = next_index;
    next_index = next_index.saturating_add(1);
    segs.push((empty_index, segment("_data_", empty_index, &[0x62u8; 16], 0, 0, 4, None)));
    // data index
    let dat_idx = next_index;
    next_index = next_index.saturating_add(1);
    let mut di = Bytes::new();
    di.u32s(recs.len());
    di.u32(0);
    for i in 0..recs.len() {
        di.u32s(data_index);
        di.u32s(i.saturating_mul(20));
        di.u32(1);
    }
    segs.push((dat_idx, segment("datidx", dat_idx, &di.b, 0, 0, 0, None)));
    // schema data and index
    let sch_dat = next_index;
    let sch_idx = next_index.saturating_add(1);
    next_index = next_index.saturating_add(2);
    let (sd, offsets, sd_names) = schema_data();
    segs.push((sch_dat, segment("schdat", sch_dat, &sd, 0, (0x30usize.saturating_add(sd_names)) >> 4, 0, None)));
    let (si, si_names) = schema_index(sch_dat, &offsets);
    segs.push((sch_idx, segment("schidx", sch_idx, &si, 0, (0x30usize.saturating_add(si_names)) >> 4, 0, None)));
    // search: schema 1 lists every record (sorted index = position), schema 0 none
    let search_idx = next_index;
    next_index = next_index.saturating_add(1);
    let mut se = Bytes::new();
    se.u32(2);
    se.u32(1);
    se.u64s(recs.len());
    for i in 0..recs.len() {
        se.u64s(i);
    }
    se.u32(1);
    se.u32(0);
    se.u32s(recs.len());
    for (i, r) in recs.iter().enumerate() {
        se.u64(crate::util::f64_to_u64_wrapping(r.0));
        se.u64(1);
        se.u64s(i);
    }
    se.u32(0);
    se.u64(0);
    se.u32(1);
    se.u32(0);
    se.u32(0);
    segs.push((search_idx, segment("search", search_idx, &se.b, 0, 0, 0, None)));
    // stream positions; the segment index goes last
    let count = next_index;
    let mut pos = 0x80usize;
    let mut where_: Vec<(usize, usize)> = vec![(0, 0); count];
    for (index, bytes) in &segs {
        if let Some(w) = where_.get_mut(*index) {
            *w = (pos, bytes.len());
        }
        pos = pos.saturating_add(bytes.len());
    }
    let seg_idx_off = pos;
    let si_size = (0x30usize.saturating_add(count.saturating_mul(12)).saturating_add(0x3F)) & !0x3F;
    if let Some(w) = where_.get_mut(1) {
        *w = (seg_idx_off, si_size);
    }
    let mut sx = Bytes::new();
    for &(off, size) in &where_ {
        sx.u64s(off);
        sx.u32s(size);
    }
    let seg_idx_bytes = segment("segidx", 1, &sx.b, 0, 0, 0, None);
    let file_size = seg_idx_off.saturating_add(seg_idx_bytes.len());
    // 24.1 file header
    let mut out = Bytes::new();
    out.u32(SIG);
    out.u32(0x80);
    out.u32(2);
    out.u32(2);
    out.u32(0);
    out.u32(1);
    out.u32s(seg_idx_off);
    out.u32(0);
    out.u32s(count);
    out.u32s(sch_idx);
    out.u32s(dat_idx);
    out.u32s(search_idx);
    out.u32(0);
    out.u32s(file_size);
    while out.p() < 0x80 {
        out.u8(0);
    }
    for (_, s) in &segs {
        out.raw(s);
    }
    out.raw(&seg_idx_bytes);
    Some(out.b)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trips_through_the_reader() {
        let sab = b"ACIS BinaryFile\x15\x00\x00\x00 data End-of-ASM-data".to_vec();
        let sec = write_acds(&[(0x2A as f64, sab.clone())]).expect("a section");
        let back = crate::acds::read_acds(&sec);
        assert_eq!(back.records.get(&42).cloned(), Some(sab));
    }
}
