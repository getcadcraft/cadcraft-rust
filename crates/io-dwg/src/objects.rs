//! Decode one object of AcDb:AcDbObjects into a [`Record`] (port of the framing part of
//! lib/dwg/objects-read.js; spec 20.1, 20.2, 20.4.1/20.4.2, 28). The type decoders live in
//! `dec_entities.rs`, `dec_objects.rs`, `table_read.rs`, `solid_read.rs`, `surface_read.rs`.
//!
//! A record mirrors the JS record: `f` holds the decoded fields (`rec.<name>` in JS; keys starting with
//! `_` are private and `plain()` skips them), `h` holds `rec._h` (handle references as absolute numbers).
//! The common fields the JS reader keeps on the record itself are typed fields here.

use std::collections::HashMap;

use crate::bits::{BitReader, Codepage, DwgError, Res, err, read_mcu, read_ms};
use crate::crc::crc8;
use crate::messages::msg;
use crate::value::{Obj, V};

/// One class of AcDb:Classes.
#[derive(Clone, Debug, Default)]
pub struct Class {
    pub num: i32,
    pub dxf_name: String,
    pub cpp_name: String,
    pub is_entity: bool,
}

/// What every decoder may read (no mutable state: objects decode independently, in parallel).
#[derive(Debug)]
pub struct Ctx {
    pub ver: u32,
    pub maint: u32,
    pub cp: Codepage,
    pub classes: HashMap<i32, Class>,
    pub strict: bool,
}

/// One EED block (28): app handle + [code, value] items.
#[derive(Clone, Debug, Default)]
pub struct Eed {
    pub app: f64,
    pub items: Vec<(i32, V)>,
}

#[derive(Clone, Debug, Default)]
pub struct Record {
    pub handle: f64,
    pub type_num: i32,
    /// DXF-ish type name (`rec.type`)
    pub typ: String,
    pub is_entity: bool,
    /// `rec.className` (classed objects only)
    pub class_name: Option<String>,
    /// decoded fields
    pub f: Obj,
    /// `rec._h`
    pub h: Obj,
    pub eed: Option<Vec<Eed>>,
    pub eed_error: Option<String>,
    pub crc_bad: bool,
    /// `rec._error` (a type decoder failed part way)
    pub error: Option<String>,
    /// `rec._unknown` (no decoder)
    pub unknown: bool,
    // ---- common entity / object data
    pub entmode: u32,
    pub nreact: i32,
    pub no_xdic: u32,
    pub ds: Option<u32>,
    pub bylayerlt: u32,
    pub nolinks: u32,
    /// `rec.color` (entities; LAYER sets it too)
    pub color: Option<Obj>,
    pub transparency: Option<u32>,
    pub color_book: bool,
    /// `rec.ltscale` (None = undefined)
    pub ltscale: Option<f64>,
    /// `rec.invisible` (None = undefined)
    pub invisible: Option<bool>,
    pub ltflags: Option<u32>,
    pub psflags: Option<u32>,
    pub matflags: Option<u32>,
    pub vs: [u32; 3],
    pub lw: Option<u32>,
    pub r14bits: u32,
    pub gfx_pos: Option<usize>,
    pub graphics: f64,
    // ---- positions (bits) for passthrough.js / mleader-read.js re-reads
    pub size: f64,
    pub data_start: usize,
    pub data_end: Option<usize>,
    pub h_pos: usize,
    pub endbit: usize,
    pub obj_end: usize,
    pub data_left: i64,
    /// R2007+: the string stream (pos, end) when found
    pub str_stream: Option<(usize, usize)>,
}

impl Record {
    /// `rec._h.<k>` as a handle number.
    pub fn hn(&self, k: &str) -> Option<f64> {
        self.h.num(k)
    }
    /// `rec._h.<k>` as a list of handles.
    pub fn hlist(&self, k: &str) -> Option<Vec<f64>> {
        self.h.arr(k).map(|a| a.iter().filter_map(V::as_f64).collect())
    }
}

pub type Decoder = fn(&mut BitReader, &mut BitReader, &mut Record, &Ctx) -> Res<()>;

/// 20.3 fixed type numbers.
pub fn fixed_type(n: i32) -> Option<&'static str> {
    Some(match n {
        1 => "TEXT",
        2 => "ATTRIB",
        3 => "ATTDEF",
        4 => "BLOCK",
        5 => "ENDBLK",
        6 => "SEQEND",
        7 => "INSERT",
        8 => "MINSERT",
        10 => "VERTEX_2D",
        11 => "VERTEX_3D",
        12 => "VERTEX_MESH",
        13 => "VERTEX_PFACE",
        14 => "VERTEX_PFACE_FACE",
        15 => "POLYLINE_2D",
        16 => "POLYLINE_3D",
        17 => "ARC",
        18 => "CIRCLE",
        19 => "LINE",
        20 => "DIMENSION_ORDINATE",
        21 => "DIMENSION_LINEAR",
        22 => "DIMENSION_ALIGNED",
        23 => "DIMENSION_ANG3PT",
        24 => "DIMENSION_ANG2LN",
        25 => "DIMENSION_RADIUS",
        26 => "DIMENSION_DIAMETER",
        27 => "POINT",
        28 => "3DFACE",
        29 => "POLYLINE_PFACE",
        30 => "POLYLINE_MESH",
        31 => "SOLID",
        32 => "TRACE",
        33 => "SHAPE",
        34 => "VIEWPORT",
        35 => "ELLIPSE",
        36 => "SPLINE",
        37 => "REGION",
        38 => "3DSOLID",
        39 => "BODY",
        40 => "RAY",
        41 => "XLINE",
        42 => "DICTIONARY",
        43 => "OLEFRAME",
        44 => "MTEXT",
        45 => "LEADER",
        46 => "TOLERANCE",
        47 => "MLINE",
        48 => "BLOCK_CONTROL",
        49 => "BLOCK_HEADER",
        50 => "LAYER_CONTROL",
        51 => "LAYER",
        52 => "STYLE_CONTROL",
        53 => "STYLE",
        56 => "LTYPE_CONTROL",
        57 => "LTYPE",
        60 => "VIEW_CONTROL",
        61 => "VIEW",
        62 => "UCS_CONTROL",
        63 => "UCS",
        64 => "VPORT_CONTROL",
        65 => "VPORT",
        66 => "APPID_CONTROL",
        67 => "APPID",
        68 => "DIMSTYLE_CONTROL",
        69 => "DIMSTYLE",
        70 => "VPENT_CONTROL",
        71 => "VPENT_HEADER",
        72 => "GROUP",
        73 => "MLINESTYLE",
        74 => "OLE2FRAME",
        75 => "DUMMY",
        76 => "LONG_TRANSACTION",
        77 => "LWPOLYLINE",
        78 => "HATCH",
        79 => "XRECORD",
        80 => "ACDBPLACEHOLDER",
        81 => "VBA_PROJECT",
        82 => "LAYOUT",
        0x1F2 => "ACAD_PROXY_ENTITY",
        0x1F3 => "ACAD_PROXY_OBJECT",
        _ => return None,
    })
}
fn entity_fixed(n: i32) -> bool {
    matches!(n, 1..=8 | 10..=41 | 43..=47 | 74 | 77 | 78 | 0x1F2)
}
fn class_alias(name: &str) -> &str {
    match name {
        "LWPOLYLINE" | "HATCH" | "XRECORD" | "LAYOUT" | "OLE2FRAME" | "ACDBPLACEHOLDER" | "GROUP" | "MLINESTYLE" | "DICTIONARYWDFLT" => {
            name
        }
        _ => name,
    }
}

/// The decoder for a type name, from every decoder module.
pub fn decoder(name: &str) -> Option<Decoder> {
    // objects-read.js: Object.assign(DECODERS, TABLE_DECODERS) comes last, so table-read.js wins
    crate::table_read::decoder(name)
        .or_else(|| crate::dec_mleader::decoder(name))
        .or_else(|| crate::dec_entities::decoder(name))
        .or_else(|| crate::dec_objects::decoder(name))
        .or_else(|| crate::solid_read::decoder(name))
        .or_else(|| crate::surface_read::decoder(name))
}

/// Frame an object at byte `offset` of `buf` and decode it.
pub fn decode_object(buf: &[u8], offset: f64, ctx: &Ctx) -> Res<Record> {
    let ver = ctx.ver;
    if offset < 0.0 || offset + 2.0 > crate::util::usize_to_f64(buf.len()) {
        return err(msg("ccdwg.err.offset_outside", &[]));
    }
    let offset = crate::util::to_usize(offset);
    let (size, mut p) = read_ms(buf, offset);
    let mut hsize = 0.0;
    if ver >= 24 {
        let (v, q) = read_mcu(buf, p);
        hsize = v;
        p = q;
    }
    if crate::util::usize_to_f64(p) + size > crate::util::usize_to_f64(buf.len()) {
        let n = crate::value::V::Num(size).to_json();
        return err(msg("ccdwg.err.object_past_end", &[("n", &n)]));
    }
    let size_u = crate::util::to_usize(size);
    let obj_start = p.saturating_mul(8);
    let obj_end = p.saturating_add(size_u).saturating_mul(8);
    let mut r = BitReader::range(buf, ver, ctx.cp, obj_start, obj_end);
    let type_num = r.ot()?;
    let mut endbit: Option<usize> = None;
    if ver >= 24 {
        let e = crate::util::usize_to_f64(obj_end) - hsize;
        if e < crate::util::usize_to_f64(obj_start) {
            return err(msg("ccdwg.err.size_fields", &[]));
        }
        endbit = Some(crate::util::to_usize(e));
    } else if ver >= 15 {
        endbit = Some(obj_start.saturating_add(crate::util::usize_from_u32(r.rl()?)));
    }
    if let Some(e) = endbit
        && (e < obj_start || e > obj_end)
    {
        return err(msg("ccdwg.err.size_fields", &[]));
    }
    let handle = r.h(0.0)?;
    let mut rec = Record { handle, type_num, size, ..Default::default() };
    let end_byte = p.saturating_add(size_u);
    if end_byte.saturating_add(2) <= buf.len() {
        let want = u32::from(buf.get(end_byte).copied().unwrap_or(0))
            | (u32::from(buf.get(end_byte.saturating_add(1)).copied().unwrap_or(0)) << 8);
        if u32::from(crc8(0xC0C1, buf, offset, end_byte)) != want {
            rec.crc_bad = true;
        }
    }
    read_eed(&mut r, &mut rec, ctx)?;

    let mut name: Option<String> = fixed_type(type_num).map(str::to_string);
    if type_num >= 500 {
        let cls = ctx.classes.get(&type_num);
        name = Some(match cls {
            Some(c) => c.dxf_name.clone(),
            None => format!("CLASS_{type_num}"),
        });
        rec.is_entity = cls.is_some_and(|c| c.is_entity);
        rec.class_name = Some(cls.map(|c| c.cpp_name.clone()).unwrap_or_default());
    } else {
        rec.is_entity = entity_fixed(type_num);
    }
    rec.typ = name.clone().unwrap_or_else(|| format!("TYPE_{type_num}"));

    if ver < 15 {
        endbit = None;
    }
    if rec.is_entity {
        common_entity(&mut r, &mut rec, ctx)?;
    } else {
        common_object(&mut r, &mut rec, ctx)?;
    }
    if ver < 15 {
        let e = obj_start.saturating_add(crate::util::usize_from_u32(rec.r14bits));
        if e < obj_start || e > obj_end {
            return err(msg("ccdwg.err.size_fields", &[]));
        }
        endbit = Some(e);
    }
    let mut data_end = endbit;
    if ver >= 21 {
        let eb = endbit.unwrap_or(obj_end);
        match string_stream_start(buf, eb, obj_start) {
            Some((s, e)) => {
                r.set_str(s, e);
                rec.str_stream = Some((s, e));
                data_end = Some(s);
            }
            None => r.set_str(eb, eb),
        }
    }
    r.end = obj_end.min(data_end.unwrap_or(obj_end));
    let mut h = BitReader::range(buf, ver, ctx.cp, endbit.unwrap_or(obj_end), obj_end);
    rec.data_end = data_end;

    let ch = if rec.is_entity { common_entity_handles(&mut h, &mut rec, ctx) } else { common_object_handles(&mut h, &mut rec) };
    if let Err(e) = ch {
        rec.error = Some(format!("handles: {}", e.0));
    }
    rec.data_start = r.pos;
    rec.h_pos = h.pos;
    rec.endbit = endbit.unwrap_or(obj_end);
    rec.obj_end = obj_end;
    let key = name.as_deref().map(class_alias).unwrap_or("");
    match decoder(key) {
        Some(dec) => {
            if let Err(e) = dec(&mut r, &mut h, &mut rec, ctx) {
                rec.error = Some(match rec.error.take() {
                    Some(prev) => format!("{prev}; {}", e.0),
                    None => e.0,
                });
            }
        }
        None => rec.unknown = true,
    }
    rec.data_left = r.bits_left();
    Ok(rec)
}

/// 20.1: the string stream of an R2007+ object/section ending at `endbit` → (start, end).
pub fn string_stream_start(buf: &[u8], endbit: usize, min_bit: usize) -> Option<(usize, usize)> {
    let mut t = BitReader::range(buf, 15, Codepage::latin1(), endbit.checked_sub(1)?, endbit);
    if t.b().ok()? == 0 {
        return None;
    }
    let mut p = endbit.checked_sub(17)?;
    if p < min_bit {
        return None;
    }
    t.pos = p;
    t.end = p.saturating_add(16); // p < endbit
    let mut size = crate::util::usize_from_u32(t.rsu().ok()?);
    if size & 0x8000 != 0 {
        p = p.checked_sub(16)?;
        t.pos = p;
        t.end = p.saturating_add(16);
        let hi = crate::util::usize_from_u32(t.rsu().ok()?);
        size = (size & 0x7FFF) | (hi << 15);
    }
    let start = p.checked_sub(size)?;
    if start < min_bit {
        return None;
    }
    Some((start, p))
}

// ---------------------------------------------------------------- 28 EED
fn read_eed(r: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let mut n = 0u32;
    loop {
        let size = r.bs()?;
        if size == 0 {
            break;
        }
        let app = r.h(0.0)?;
        let end_f = crate::util::usize_to_f64(r.pos) + f64::from(size) * 8.0;
        let end = crate::util::to_usize(end_f);
        let mut items: Vec<(i32, V)> = Vec::new();
        let res: Res<()> = (|| {
            while crate::util::usize_to_f64(r.pos) < end_f {
                let code = r.rc()?;
                match code {
                    0 => {
                        if ctx.ver >= 21 {
                            let len = r.rsu()?;
                            let mut u = Vec::with_capacity(crate::util::usize_from_u32(len));
                            for _ in 0..len {
                                u.push(u16::try_from(r.rsu()?).unwrap_or(0));
                            }
                            items.push((1000, V::Str(crate::bits::from_utf16(&u))));
                        } else {
                            let len = r.rc()?;
                            r.rsu()?;
                            let b = r.bytes(crate::util::usize_from_u32(len))?;
                            items.push((1000, V::Str(ctx.cp.decode(&b))));
                        }
                    }
                    2 => items.push((1002, V::from(if r.rc()? != 0 { "}" } else { "{" }))),
                    3 | 5 => {
                        let b = r.bytes(8)?;
                        let mut v = 0.0f64;
                        for x in b {
                            v = v * 256.0 + f64::from(x);
                        }
                        items.push((i32::try_from(code).unwrap_or(0).saturating_add(1000), V::Str(crate::util::hex_of(v))));
                    }
                    4 => {
                        let len = r.rc()?;
                        let b = r.bytes(crate::util::usize_from_u32(len))?;
                        items.push((1004, V::Str(crate::util::hex_bytes(&b))));
                    }
                    10..=15 => items.push((i32::try_from(code).unwrap_or(0).saturating_add(1000), r.rd3()?)),
                    40..=42 => items.push((i32::try_from(code).unwrap_or(0).saturating_add(1000), V::Num(r.rd()?))),
                    70 => items.push((1070, V::from(r.rs()?))),
                    71 => items.push((1071, V::from(r.rls()?))),
                    _ => return err(msg("ccdwg.err.eed_code", &[("code", &code.to_string())])),
                }
            }
            Ok(())
        })();
        if let Err(e) = res {
            rec.eed_error = Some(e.0);
        }
        r.pos = end;
        rec.eed.get_or_insert_with(Vec::new).push(Eed { app, items });
        // A negative size moves the JS reader before the buffer, where every read gives 0: its next size is 0 and
        // the loop ends. Stop here too (re-reading from 0 would cycle 10 000 times; found by fuzzing).
        if end_f < 0.0 {
            break;
        }
        n = n.saturating_add(1);
        if n > 10000 {
            break;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- 20.2 / 20.4.1 common entity data
fn common_entity(r: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let ver = ctx.ver;
    if r.b()? != 0 {
        let n = if ver >= 24 { r.bll()? } else { f64::from(r.rl()?) };
        rec.gfx_pos = Some(r.pos);
        r.pos = crate::util::to_usize(crate::util::usize_to_f64(r.pos) + n * 8.0);
        rec.graphics = n;
    }
    if ver < 15 {
        rec.r14bits = r.rl()?;
    }
    rec.entmode = r.bb()?;
    rec.nreact = r.bl()?;
    rec.no_xdic = if ver >= 18 { r.b()? } else { 0 };
    if ver >= 27 {
        rec.ds = Some(r.b()?);
    }
    if ver < 15 {
        rec.bylayerlt = r.b()?;
    }
    rec.nolinks = if ver <= 15 { r.b()? } else { 1 };
    let (color, tr, book) = r.enc()?;
    rec.color = Some(color);
    rec.transparency = tr;
    rec.color_book = book;
    rec.ltscale = Some(r.bd()?);
    if ver >= 15 {
        rec.ltflags = Some(r.bb()?);
        rec.psflags = Some(r.bb()?);
    }
    if ver >= 21 {
        rec.matflags = Some(r.bb()?);
        r.rc()?;
    }
    if ver >= 24 {
        rec.vs = [r.b()?, r.b()?, r.b()?];
    }
    rec.invisible = Some(r.bs()? & 1 != 0);
    if ver >= 15 {
        rec.lw = Some(r.rc()?);
    }
    Ok(())
}

fn common_entity_handles(h: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let ver = ctx.ver;
    let hd = rec.handle;
    if rec.entmode == 0 {
        rec.h.set("owner", h.h(hd)?);
    }
    if rec.nreact != 0 {
        let a = hrefs(h, hd, f64::from(rec.nreact))?;
        rec.h.set("reactors", a);
    }
    if rec.no_xdic == 0 {
        rec.h.set("xdic", h.h(hd)?);
    }
    if ver < 15 {
        rec.h.set("layer", h.h(hd)?);
        if rec.bylayerlt == 0 {
            rec.h.set("ltype", h.h(hd)?);
        }
    }
    if ver <= 15 && rec.nolinks == 0 {
        rec.h.set("prev", h.h(hd)?);
        rec.h.set("next", h.h(hd)?);
    }
    if ver >= 18 && rec.color_book {
        rec.h.set("colorBook", h.h(hd)?);
    }
    if ver >= 15 {
        rec.h.set("layer", h.h(hd)?);
        if rec.ltflags == Some(3) {
            rec.h.set("ltype", h.h(hd)?);
        }
        if ver >= 21 && rec.matflags == Some(3) {
            rec.h.set("material", h.h(hd)?);
        }
        if rec.psflags == Some(3) {
            rec.h.set("plotstyle", h.h(hd)?);
        }
        if ver >= 24 {
            if rec.vs[0] != 0 {
                rec.h.set("vsFull", h.h(hd)?);
            }
            if rec.vs[1] != 0 {
                rec.h.set("vsFace", h.h(hd)?);
            }
            if rec.vs[2] != 0 {
                rec.h.set("vsEdge", h.h(hd)?);
            }
        }
    }
    Ok(())
}

fn common_object(r: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let ver = ctx.ver;
    if ver < 15 {
        rec.r14bits = r.rl()?;
    }
    rec.nreact = r.bl()?;
    rec.no_xdic = if ver >= 18 { r.b()? } else { 0 };
    if ver >= 27 {
        rec.ds = Some(r.b()?);
    }
    Ok(())
}
fn common_object_handles(h: &mut BitReader, rec: &mut Record) -> Res<()> {
    let hd = rec.handle;
    rec.h.set("owner", h.h(hd)?);
    if rec.nreact != 0 {
        let a = hrefs(h, hd, f64::from(rec.nreact))?;
        rec.h.set("reactors", a);
    }
    if rec.no_xdic == 0 {
        rec.h.set("xdic", h.h(hd)?);
    }
    Ok(())
}

// ---------------------------------------------------------------- helpers for the decoders

/// n handle references relative to `handle` (a count larger than the stream fails at once).
pub fn hrefs(h: &mut BitReader, handle: f64, n: f64) -> Res<Vec<V>> {
    if n < 0.0 || n * 8.0 > crate::util::i64_to_f64(h.bits_left()) {
        let s = V::Num(n).to_json();
        return err(msg("ccdwg.err.handle_count", &[("n", &s)]));
    }
    let n = crate::util::to_usize(n);
    let mut a = Vec::with_capacity(n);
    for _ in 0..n {
        a.push(V::Num(h.h(handle)?));
    }
    Ok(a)
}
/// The same as plain numbers.
pub fn hrefs_f64(h: &mut BitReader, handle: f64, n: f64) -> Res<Vec<f64>> {
    Ok(hrefs(h, handle, n)?.iter().filter_map(V::as_f64).collect())
}
/// A count of items of at least `min_bits` each must fit in what is left of the stream.
pub fn fits(r: &BitReader, n: f64, min_bits: f64) -> Res<f64> {
    if n < 0.0 || n * min_bits > crate::util::i64_to_f64(r.bits_left()) {
        let s = V::Num(n).to_json();
        return err(msg("ccdwg.err.count_large", &[("n", &s)]));
    }
    Ok(n)
}
/// Handles until fewer than 8 bits are left (or `max`).
pub fn rest_handles(h: &mut BitReader, handle: f64, max: usize) -> Vec<V> {
    let mut a = Vec::new();
    while h.bits_left() >= 8 && a.len() < max {
        match h.h(handle) {
            Ok(v) => a.push(V::Num(v)),
            Err(_) => break,
        }
    }
    a
}
/// Table entry header: name, 64-flag, xref index (before R2007), xdep.
pub fn table_entry(r: &mut BitReader, rec: &mut Record, ctx: &Ctx) -> Res<()> {
    let name = r.tv()?;
    rec.f.set("name", name);
    let f64flag = r.b()?;
    rec.f.set("_flag64", f64flag);
    if ctx.ver < 21 {
        let x = r.bs()?;
        rec.f.set("_xrefIndex", x.wrapping_sub(1));
    }
    let xdep = r.b()?;
    rec.f.set("_xdep", xdep);
    Ok(())
}
pub fn table_entry_handles(h: &mut BitReader, rec: &mut Record) -> Res<()> {
    let v = h.h(rec.handle)?;
    rec.h.set("xrefBlock", v);
    Ok(())
}

impl From<DwgError> for String {
    fn from(e: DwgError) -> String {
        e.0
    }
}
