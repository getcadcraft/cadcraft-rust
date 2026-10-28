//! las.js — LiDAR point clouds: LAS 1.0–1.4 read here, LAZ through the `laz` crate (laz-rs, Apache-2.0; the JS uses
//! laz-perf). Files are hostile: every size, count and offset is checked before it is used, and the LAZ decoder runs
//! under a panic guard (a decoder panic on a damaged file ends the scan with the JS's "stopped" note).

use crate::crs;
use crate::raster::{InFile, locale_int, too_big};
use crate::terrain::{self, limits};
use cc_civil::i18n::t;
use cc_civil::js::J;
use cc_civil::tr;
use regex::Regex;
use serde_json::{Map, Value, json};
use std::sync::OnceLock;

type R<T> = Result<T, String>;

/// `className(c)`.
pub fn class_name(c: f64) -> Option<String> {
    let i = cc_civil::js::usize_of(c).filter(|i| *i <= 18 && c == (*i as f64))?;
    Some(t(&format!("las.class.{i}"), &[]))
}
const MIN_LEN: [f64; 11] = [20.0, 28.0, 26.0, 34.0, 57.0, 63.0, 30.0, 36.0, 38.0, 59.0, 67.0];

fn u8_at(b: &[u8], o: f64) -> R<u8> {
    cc_civil::js::usize_of(o).and_then(|i| b.get(i)).copied().ok_or_else(oob)
}
fn arr_at<const N: usize>(b: &[u8], o: f64) -> R<[u8; N]> {
    let i = cc_civil::js::usize_of(o).ok_or_else(oob)?;
    let s = i.checked_add(N).and_then(|e| b.get(i..e)).ok_or_else(oob)?;
    let mut a = [0u8; N];
    a.copy_from_slice(s);
    Ok(a)
}
fn oob() -> String {
    "Offset is outside the bounds of the DataView".into()
}
fn u16_at(b: &[u8], o: f64) -> R<f64> {
    Ok(f64::from(u16::from_le_bytes(arr_at(b, o)?)))
}
fn u32_at(b: &[u8], o: f64) -> R<f64> {
    Ok(f64::from(u32::from_le_bytes(arr_at(b, o)?)))
}
fn i32_at(b: &[u8], o: f64) -> R<f64> {
    Ok(f64::from(i32::from_le_bytes(arr_at(b, o)?)))
}
fn f64_at(b: &[u8], o: f64) -> R<f64> {
    Ok(f64::from_le_bytes(arr_at(b, o)?))
}
fn u64_at(b: &[u8], o: f64) -> R<f64> {
    Ok(u32_at(b, o + 4.0)? * 4294967296.0 + u32_at(b, o)?)
}
/// `ascii(dv, o, n)`.
fn ascii(b: &[u8], o: f64, n: usize) -> R<String> {
    let mut s = String::new();
    for i in 0..n {
        let c = u8_at(b, o + i as f64)?;
        if c == 0 {
            break;
        }
        s.push(if (32..127).contains(&c) { char::from(c) } else { ' ' });
    }
    Ok(cc_civil::js::trim(&s).to_string())
}
/// `file.slice(a, b)` → bytes (clamped as Blob.slice clamps).
fn slice(b: &[u8], a: f64, e: f64) -> &[u8] {
    let len = b.len() as f64;
    let s = a.clamp(0.0, len);
    let e = e.clamp(0.0, len).max(s);
    b.get(terrain::count_of(s.trunc())..terrain::count_of(e.trunc())).unwrap_or(&[])
}

/// A variable-length record's place.
#[derive(Clone, Debug, PartialEq)]
pub struct Vlr {
    pub user: String,
    pub id: f64,
    pub desc: String,
    pub offset: f64,
    pub length: f64,
    pub ext: bool,
}
impl J for Vlr {
    fn j(&self) -> Value {
        json!({ "user": self.user, "id": self.id.j(), "desc": self.desc, "offset": self.offset.j(), "length": self.length.j(), "ext": self.ext })
    }
}

/// `readLasHeader(file)`.
#[derive(Clone, Debug)]
pub struct Header {
    pub name: String,
    pub size: f64,
    pub version: String,
    pub format: f64,
    pub record_length: f64,
    pub count: f64,
    pub available: f64,
    pub offset_to_points: f64,
    pub header_size: f64,
    pub scale: [f64; 3],
    pub offset: [f64; 3],
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub vlrs: Vec<Vlr>,
    pub evlrs: Vec<Vlr>,
    pub compressed: bool,
    pub notes: Vec<String>,
    pub crs: Value,
    pub code: Option<f64>,
    pub crs_note: Option<String>,
    pub z_unit: Option<&'static str>,
    pub wkt: Option<String>,
    /// the header's VLR count ran past the start of the points (laz-perf, reading the records itself, cannot open
    /// such a LAZ file)
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    vlr_overrun: bool,
}
impl J for Header {
    fn j(&self) -> Value {
        let mut o = Map::new();
        o.insert("name".into(), self.name.clone().into());
        o.insert("size".into(), self.size.j());
        o.insert("version".into(), self.version.clone().into());
        o.insert("format".into(), self.format.j());
        o.insert("recordLength".into(), self.record_length.j());
        o.insert("count".into(), self.count.j());
        o.insert("available".into(), self.available.j());
        o.insert("offsetToPoints".into(), self.offset_to_points.j());
        o.insert("headerSize".into(), self.header_size.j());
        o.insert("scale".into(), self.scale.to_vec().j());
        o.insert("offset".into(), self.offset.to_vec().j());
        o.insert("min".into(), self.min.to_vec().j());
        o.insert("max".into(), self.max.to_vec().j());
        o.insert("vlrs".into(), self.vlrs.j());
        o.insert("evlrs".into(), self.evlrs.j());
        o.insert("compressed".into(), self.compressed.into());
        o.insert("notes".into(), self.notes.j());
        o.insert("crs".into(), self.crs.clone());
        o.insert("code".into(), self.code.j());
        o.insert("crsNote".into(), self.crs_note.j());
        o.insert("zUnit".into(), self.z_unit.map(str::to_string).j());
        o.insert("wkt".into(), self.wkt.j());
        Value::Object(o)
    }
}

fn bad(key: &str) -> String {
    t(key, &[])
}

/// `readLasHeader(file)`.
pub fn read_las_header(file: &InFile) -> R<Header> {
    read_las_header_with(file, true)
}

/// `readLasHeader(lenient(file, 200 MB))` (pointcloud.js `lasInfo`): a viewer keeps only a budget of points, so the
/// terrain limit (200 MB) does not apply; the caller checks its own (2000 MB).
pub fn read_las_header_lenient(file: &InFile) -> R<Header> {
    read_las_header_with(file, false)
}

fn read_las_header_with(file: &InFile, terrain_limit: bool) -> R<Header> {
    let b = &file.bytes[..];
    let size = b.len() as f64;
    if terrain_limit && size > limits::FILE_BYTES {
        return Err(too_big(&file.name, size));
    }
    if size < 227.0 {
        return Err(bad("las.bad.short"));
    }
    let hb = slice(b, 0.0, 375.0);
    if ascii(hb, 0.0, 4)? != "LASF" {
        return Err(t("las.not_las", &[]));
    }
    let major = u8_at(hb, 24.0)?;
    let minor = u8_at(hb, 25.0)?;
    if major != 1 || minor > 4 {
        return Err(tr!("las.bad.version", version = format!("{major}.{minor}")));
    }
    let header_size = u16_at(hb, 94.0)?;
    let offset_to_points = u32_at(hb, 96.0)?;
    let n_vlr = u32_at(hb, 100.0)?;
    let fmt_byte = u8_at(hb, 104.0)?;
    let format = f64::from(fmt_byte & 0x3f);
    let compressed = (fmt_byte & 0xc0) != 0;
    let record_length = u16_at(hb, 105.0)?;
    if header_size < 227.0 || header_size > size {
        return Err(bad("las.bad.header_size"));
    }
    if offset_to_points < header_size || offset_to_points > size {
        return Err(bad("las.bad.offset"));
    }
    if format > 10.0 {
        return Err(tr!("las.bad.format", format = format));
    }
    let min_len = cc_civil::js::usize_of(format).and_then(|i| MIN_LEN.get(i)).copied().unwrap_or(f64::NAN);
    if record_length < min_len {
        return Err(tr!("las.bad.record_len", len = record_length, format = format));
    }
    if n_vlr > 100000.0 {
        return Err(tr!("las.bad.vlr_count", n = n_vlr));
    }
    let mut count = u32_at(hb, 107.0)?;
    let scale = [f64_at(hb, 131.0)?, f64_at(hb, 139.0)?, f64_at(hb, 147.0)?];
    let offset = [f64_at(hb, 155.0)?, f64_at(hb, 163.0)?, f64_at(hb, 171.0)?];
    let max = [f64_at(hb, 179.0)?, f64_at(hb, 195.0)?, f64_at(hb, 211.0)?];
    let min = [f64_at(hb, 187.0)?, f64_at(hb, 203.0)?, f64_at(hb, 219.0)?];
    if !scale.iter().chain(offset.iter()).all(|v| v.is_finite()) || scale.iter().any(|s| *s == 0.0 || s.abs() > 1e6) {
        return Err(bad("las.bad.scale"));
    }
    let (mut evlr_start, mut n_evlr) = (0.0, 0.0);
    if minor >= 4 && header_size >= 375.0 {
        evlr_start = u64_at(hb, 235.0)?;
        n_evlr = u32_at(hb, 243.0)?;
        let c64 = u64_at(hb, 247.0)?;
        if c64 > 0.0 {
            count = c64;
        }
    }
    let mut notes = Vec::new();
    let mut vlrs = Vec::new();
    let mut vlr_overrun = false;
    let mut o = header_size;
    let vb = slice(b, header_size, offset_to_points);
    let vlen = vb.len() as f64;
    let mut i = 0.0;
    while i < n_vlr {
        let r = o - header_size;
        if r + 54.0 > vlen {
            notes.push(tr!("las.vlr.past", n = n_vlr - i));
            vlr_overrun = true;
            break;
        }
        let len = u16_at(vb, r + 20.0)?;
        if r + 54.0 + len > vlen {
            notes.push(t("las.vlr.one_past", &[]));
            vlr_overrun = true;
            break;
        }
        vlrs.push(Vlr {
            user: ascii(vb, r + 2.0, 16)?,
            id: u16_at(vb, r + 18.0)?,
            desc: ascii(vb, r + 22.0, 32)?,
            offset: o + 54.0,
            length: len,
            ext: false,
        });
        o += 54.0 + len;
        i += 1.0;
    }
    let mut evlrs = Vec::new();
    if n_evlr != 0.0 && evlr_start != 0.0 {
        if n_evlr > 100000.0 || evlr_start < offset_to_points || evlr_start > size {
            notes.push(t("las.evlr.outside", &[]));
        } else {
            let mut e = evlr_start;
            let mut i = 0.0;
            while i < n_evlr && e + 60.0 <= size {
                let eb = slice(b, e, e + 60.0);
                let len = u64_at(eb, 20.0)?;
                if e + 60.0 + len > size {
                    notes.push(t("las.evlr.past", &[]));
                    break;
                }
                evlrs.push(Vlr {
                    user: ascii(eb, 2.0, 16)?,
                    id: u16_at(eb, 18.0)?,
                    desc: ascii(eb, 22.0, 32)?,
                    offset: e + 60.0,
                    length: len,
                    ext: true,
                });
                e += 60.0 + len;
                i += 1.0;
            }
        }
    }
    let end = if evlr_start != 0.0 && evlr_start > offset_to_points && evlr_start <= size { evlr_start } else { size };
    let mut available = count;
    if !compressed {
        available = ((end - offset_to_points) / record_length).floor();
        if available < count {
            notes.push(tr!("las.short", available = locale_int(available), count = locale_int(count)));
        } else {
            available = count;
        }
    } else if !vlrs.iter().any(|v| v.user == "laszip encoded" && v.id == 22204.0) {
        return Err(bad("las.bad.no_laszip"));
    }
    let mut h = Header {
        name: file.name.clone(),
        size,
        version: format!("{major}.{minor}"),
        format,
        record_length,
        count,
        available,
        offset_to_points,
        header_size,
        scale,
        offset,
        min,
        max,
        vlrs,
        evlrs,
        compressed,
        notes,
        crs: Value::Null,
        code: None,
        crs_note: None,
        z_unit: None,
        wkt: None,
        vlr_overrun,
    };
    las_crs(b, &mut h)?;
    Ok(h)
}

fn vlr_bytes<'a>(b: &'a [u8], v: &Vlr, max: f64) -> Option<&'a [u8]> {
    if v.length > max {
        return None;
    }
    Some(slice(b, v.offset, v.offset + v.length))
}

fn las_crs(b: &[u8], h: &mut Header) -> R<()> {
    let all: Vec<Vlr> = h.vlrs.iter().chain(h.evlrs.iter()).filter(|v| v.user == "LASF_Projection").cloned().collect();
    if let Some(wv) = all.iter().find(|v| v.id == 2112.0) {
        let text = match vlr_bytes(b, wv, 100000.0) {
            Some(bytes) => {
                let s = String::from_utf8_lossy(bytes);
                cc_civil::js::trim(s.trim_end_matches('\u{0}')).to_string()
            }
            None => String::new(),
        };
        h.wkt = if text.is_empty() { None } else { Some(text.clone()) };
        if !text.is_empty() {
            match crs::crs_from_prj(&text) {
                Ok(r) => {
                    h.crs = r.crs;
                    h.code = r.matched.map(|e| e.code);
                    let j = r.notes.join(" ");
                    h.crs_note = if j.is_empty() { None } else { Some(j) };
                }
                Err(e) => h.crs_note = Some(if e.is_empty() { t("las.crs.bad", &[]) } else { tr!("las.crs.bad_why", reason = e) }),
            }
            static P: OnceLock<[Regex; 2]> = OnceLock::new();
            let p = P.get_or_init(|| [rx(r#"(?is)VERT_?CS\[.*?UNIT\["([^"]+)""#), rx(r#"(?is)VERTCRS\[.*?LENGTHUNIT\["([^"]+)""#)]);
            let vu = p[0].captures(&text).or_else(|| p[1].captures(&text)).and_then(|c| c.get(1)).map(|m| m.as_str().to_string());
            if let Some(u) = vu {
                h.z_unit = terrain::unit_from_name(&u);
            }
            if !h.crs.is_null() {
                return Ok(());
            }
        }
    }
    if let Some(kv) = all.iter().find(|v| v.id == 34735.0)
        && let Some(kb) = vlr_bytes(b, kv, 65536.0).filter(|k| k.len() >= 8)
    {
        let n = u16_at(kb, 6.0)?;
        let mut keys = Map::new();
        let dbl = all.iter().find(|v| v.id == 34736.0).and_then(|v| vlr_bytes(b, v, 65536.0));
        let klen = kb.len() as f64;
        let mut i = 0.0;
        while i < n && 8.0 + 8.0 * i + 8.0 <= klen {
            let id = u16_at(kb, 8.0 + 8.0 * i)?;
            let loc = u16_at(kb, 10.0 + 8.0 * i)?;
            let val = u16_at(kb, 14.0 + 8.0 * i)?;
            let key = cc_civil::js::num_to_string(id);
            if loc == 0.0 {
                keys.insert(key, val.j());
            } else if loc == 34736.0
                && let Some(d) = dbl
                && (val + 1.0) * 8.0 <= d.len() as f64
            {
                keys.insert(key, f64_at(d, val * 8.0)?.j());
            }
            i += 1.0;
        }
        let g = terrain::crs_from_geo_keys(Some(&keys));
        h.crs = g.crs;
        h.code = g.code;
        h.crs_note = g.note;
        h.z_unit = h.z_unit.or(g.z_unit);
        return Ok(());
    }
    if h.crs.is_null() && h.crs_note.is_none() {
        h.crs_note = Some(t("las.crs.none", &[]));
    }
    Ok(())
}

fn rx(p: &str) -> Regex {
    Regex::new(p).unwrap_or_else(|_| never())
}
#[allow(clippy::expect_used)]
fn never() -> Regex {
    Regex::new("a^").expect("a constant pattern")
}

fn decode(h: &Header, rec: &[u8], o: f64, on_point: &mut dyn FnMut(f64, f64, f64, f64)) -> R<()> {
    let ext = h.format >= 6.0;
    let x = i32_at(rec, o)? * h.scale[0] + h.offset[0];
    let y = i32_at(rec, o + 4.0)? * h.scale[1] + h.offset[1];
    let z = i32_at(rec, o + 8.0)? * h.scale[2] + h.offset[2];
    let cls = if ext { f64::from(u8_at(rec, o + 16.0)?) } else { f64::from(u8_at(rec, o + 15.0)? & 31) };
    on_point(x, y, z, cls);
    Ok(())
}

/// `scanPoints(file, header, onPoint)` → points read (LAZ may add a note to the header).
pub fn scan_points(file: &InFile, h: &mut Header, on_point: &mut dyn FnMut(f64, f64, f64, f64)) -> R<f64> {
    if h.compressed { scan_laz(file, h, on_point) } else { scan_las(file, h, on_point) }
}

/// Every point record's bytes in file order (pointcloud.js `scanLas` reads intensity, class and colour besides x y z,
/// so it decodes the records itself): a LAS file 4 MB of whole records at a time (a short last piece ends the scan
/// after its whole records), a LAZ file through the decompressor — `min(count, …)` points, a point under 20 bytes
/// refused, a decoder that stops adding the "stopped" note to `notes`. → records read.
pub fn scan_records(file: &InFile, h: &Header, notes: &mut Vec<String>, on_rec: &mut dyn FnMut(&[u8]) -> R<()>) -> R<f64> {
    if h.compressed {
        return scan_laz_records(file, h, notes, 20.0, on_rec);
    }
    let l = h.record_length;
    let per = 1.0f64.max((4.0 * 1048576.0 / l).floor());
    let total = h.available;
    let mut read = 0.0;
    let mut i = 0.0;
    let len = terrain::count_of(l);
    while i < total {
        let n = per.min(total - i);
        let start = h.offset_to_points + i * l;
        let dv = slice(&file.bytes, start, start + n * l);
        let m = (dv.len() as f64 / l).floor();
        if len > 0 {
            for rec in dv.chunks_exact(len) {
                on_rec(rec)?;
            }
        }
        read += m;
        if m < n {
            break;
        }
        i += per;
    }
    Ok(read)
}

fn scan_las(file: &InFile, h: &Header, on_point: &mut dyn FnMut(f64, f64, f64, f64)) -> R<f64> {
    let l = h.record_length;
    let per = 1.0f64.max((2.0 * 1048576.0 / l).floor());
    let mut read = 0.0;
    let mut i = 0.0;
    while i < h.available {
        let n = per.min(h.available - i);
        let start = h.offset_to_points + i * l;
        let dv = slice(&file.bytes, start, start + n * l);
        if (dv.len() as f64) < n * l {
            break;
        }
        let mut k = 0.0;
        while k < n {
            decode(h, dv, k * l, on_point)?;
            k += 1.0;
        }
        read += n;
        i += per;
    }
    Ok(read)
}

/// The chunk table laz-rs is about to read (where the 8-byte offset at the start of the points, or at the end of the
/// file, says) claims no more chunks than the file has bytes: laz-rs allocates the table from that count before it reads
/// a single entry (fuzz find: a 23 KB file asking for 2^32 entries).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn chunk_table_sane(b: &[u8], start: f64) -> bool {
    let i64_at = |o: usize| b.get(o..o.saturating_add(8)).and_then(|s| <[u8; 8]>::try_from(s).ok()).map(i64::from_le_bytes);
    let pos = terrain::count_of(start);
    let Ok(pos_i) = i64::try_from(pos) else { return false };
    let mut off = i64_at(pos).unwrap_or(0);
    if off <= pos_i {
        off = b.len().checked_sub(8).and_then(i64_at).unwrap_or(0);
        if off <= pos_i {
            return true; // no table: laz-rs reads the points without one
        }
    }
    let Ok(o) = usize::try_from(off) else { return true };
    match b.get(o.saturating_add(4)..o.saturating_add(8)).and_then(|s| <[u8; 4]>::try_from(s).ok()) {
        // every entry takes some of the bytes after the table's start
        Some(n) => (u32::from_le_bytes(n) as usize) <= b.len().saturating_sub(o),
        None => true, // past the end: laz-rs fails to read it
    }
}

/// How many layer sizes (u32 each) a layered LAZ chunk (LAS 1.4 point formats, compressor v3/v4) lists after its raw first
/// point and its point count, one per layer of each item; None for the older point-wise items (no layers).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn layer_words(lv: &laz::LazVlr) -> Option<usize> {
    let mut k = 0usize;
    for it in lv.items() {
        let w = match it.item_type() {
            laz::LazItemType::Point14 => 9,
            laz::LazItemType::RGB14 | laz::LazItemType::WavePacket14 => 1,
            laz::LazItemType::RGBNIR14 => 2,
            laz::LazItemType::Byte14(n) => usize::from(n),
            _ => return None,
        };
        k = k.checked_add(w)?;
    }
    Some(k)
}

/// The chunk at `pos` of a layered LAZ file: its layer sizes all fit in the bytes after its header, and so does their
/// sum. laz-rs sizes a buffer to each before reading it, so a forged size is a 4 GB allocation (fuzz 2026-10-17,
/// `civil_las` oom-56ae06ee). Returns where the next chunk starts.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn layered_chunk_end(b: &[u8], pos: usize, record: usize, words: usize) -> Option<usize> {
    let header = pos.checked_add(record)?.checked_add(4)?;
    let data = header.checked_add(words.checked_mul(4)?)?;
    let sizes = b.get(header..data)?;
    let room = b.len().checked_sub(data)?;
    let mut sum = 0usize;
    for w in sizes.as_chunks::<4>().0 {
        let v = usize::try_from(u32::from_le_bytes(*w)).ok()?;
        if v > room {
            return None;
        }
        sum = sum.checked_add(v)?;
    }
    (sum <= room).then(|| data.saturating_add(sum))
}

// The laz crate can still panic on a hostile file (fuzzing found two; both guarded, more may exist). Natively the
// catch_unwind guards below turn that into an error; in the browser a panic aborts the page and loses the user's
// work (AGENTS § 4), so the web build does not decode LAZ until a decoder that cannot panic is in place.
#[cfg(target_arch = "wasm32")]
fn scan_laz_records(_file: &InFile, h: &Header, notes: &mut Vec<String>, _min: f64, _on_rec: &mut dyn FnMut(&[u8]) -> R<()>) -> R<f64> {
    notes.push(tr!("las.laz.stopped", read = locale_int(0.0), n = locale_int(h.count)));
    Err(bad("las.bad.laz_open"))
}

fn scan_laz(file: &InFile, h: &mut Header, on_point: &mut dyn FnMut(f64, f64, f64, f64)) -> R<f64> {
    let min = cc_civil::js::usize_of(h.format).and_then(|i| MIN_LEN.get(i)).copied().unwrap_or(f64::NAN);
    let mut notes = Vec::new();
    let hh: &Header = h;
    let r = scan_laz_records(file, hh, &mut notes, min, &mut |rec| decode(hh, rec, 0.0, on_point));
    h.notes.append(&mut notes);
    r
}

/// The LAZ points' records (laz-rs); `min`: the shortest point the caller takes (`las.bad.laz_len` below it).
#[cfg(not(target_arch = "wasm32"))]
fn scan_laz_records(file: &InFile, h: &Header, notes: &mut Vec<String>, min: f64, on_rec: &mut dyn FnMut(&[u8]) -> R<()>) -> R<f64> {
    let b = &file.bytes[..];
    // laz-perf reads the LASzip record and the chunk table itself; laz-rs is given them
    let vlr = h.vlrs.iter().find(|v| v.user == "laszip encoded" && v.id == 22204.0).cloned().ok_or_else(|| bad("las.bad.laz_open"))?;
    let vbytes = slice(b, vlr.offset, vlr.offset + vlr.length).to_vec();
    // the decompressor reads the chunk table offset where the points start and seeks to it (a file offset)
    let start = terrain::count_of(h.offset_to_points) as u64;
    let opened = std::panic::catch_unwind(|| -> Option<(laz::LazVlr, usize)> {
        let v = laz::LazVlr::from_buffer(&vbytes).ok()?;
        let len = usize::try_from(v.items_size()).ok()?;
        Some((v, len))
    });
    let Ok(Some((lv, len))) = opened else { return Err(bad("las.bad.laz_open")) };
    // a LASzip point takes at least a few bits: a count far past what the bytes can hold is a damaged header (the
    // bound keeps a decoder that pads past the end from running for billions of points)
    let n = h.count.min(h.size * 64.0);
    if (len as f64) < min || min.is_nan() {
        return Err(bad("las.bad.laz_len"));
    }
    if h.vlr_overrun || !chunk_table_sane(b, h.offset_to_points) {
        return Err(bad("las.bad.laz_open"));
    }
    let mut cursor = std::io::Cursor::new(b);
    cursor.set_position(start);
    let dec = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| laz::LasZipDecompressor::new(cursor, lv.clone())));
    let Ok(Ok(mut dec)) = dec else { return Err(bad("las.bad.laz_open")) };
    let mut buf = vec![0u8; len];
    let mut read = 0.0;
    let mut i = 0.0;
    // layered chunks: check each chunk's layer sizes before laz-rs reads them (it allocates from them first); a chunk
    // holds `chunk_size` points (fixed) or what the chunk table says (variable), and starts where the last one's layers end
    let words = layer_words(&lv);
    let per_chunk: Option<Vec<u64>> = match (words, lv.uses_variable_size_chunks()) {
        (Some(_), true) => {
            let mut c = std::io::Cursor::new(b);
            c.set_position(start);
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| laz::laszip::ChunkTable::read_from(&mut c, &lv))) {
                Ok(Ok(t)) => Some(t.as_ref().iter().map(|e| e.point_count).collect()),
                _ => return Err(bad("las.bad.laz_open")),
            }
        }
        _ => None,
    };
    let fixed = u64::from(lv.chunk_size()).max(1);
    let mut chunk_at = usize::try_from(start).ok().and_then(|s| s.checked_add(8));
    let mut chunk_index = 0usize;
    let mut left_in_chunk = 0u64;
    while i < n {
        if let Some(k) = words
            && left_in_chunk == 0
        {
            let next = chunk_at.and_then(|at| layered_chunk_end(b, at, len, k));
            let Some(next) = next else {
                notes.push(tr!("las.laz.stopped", read = locale_int(i), n = locale_int(n)));
                break;
            };
            chunk_at = Some(next);
            left_in_chunk = match &per_chunk {
                Some(t) => t.get(chunk_index).copied().unwrap_or(0).max(1),
                None => fixed,
            };
            chunk_index = chunk_index.saturating_add(1);
        }
        left_in_chunk = left_in_chunk.saturating_sub(1);
        let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| dec.decompress_one(&mut buf)));
        if !matches!(ok, Ok(Ok(()))) {
            notes.push(tr!("las.laz.stopped", read = locale_int(i), n = locale_int(n)));
            break;
        }
        on_rec(&buf)?;
        read += 1.0;
        i += 1.0;
    }
    Ok(read)
}

#[cfg(test)]
mod layered_tests {
    use super::layered_chunk_end;

    /// A chunk: `record` raw bytes, the point count, the layer sizes, then `data` bytes of layers.
    fn chunk(record: usize, sizes: &[u32], data: usize) -> Vec<u8> {
        let mut b = vec![0u8; record];
        b.extend_from_slice(&7u32.to_le_bytes());
        for s in sizes {
            b.extend_from_slice(&s.to_le_bytes());
        }
        b.extend(std::iter::repeat_n(0u8, data));
        b
    }

    #[test]
    fn layer_sizes_must_fit_the_file() {
        let ok = chunk(30, &[10, 0, 5], 15);
        assert_eq!(layered_chunk_end(&ok, 0, 30, 3), Some(ok.len()));
        // a forged 4 GB layer (the fuzz find), a sum past the end, a header past the end
        assert_eq!(layered_chunk_end(&chunk(30, &[u32::MAX, 0, 0], 15), 0, 30, 3), None);
        assert_eq!(layered_chunk_end(&chunk(30, &[10, 10, 10], 15), 0, 30, 3), None);
        assert_eq!(layered_chunk_end(&ok, 40, 30, 3), None);
    }
}
