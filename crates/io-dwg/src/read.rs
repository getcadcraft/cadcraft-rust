//! readDwg (port of lib/dwg/read.js): containers → header (ch. 9) → classes (ch. 10) → object map (ch. 23)
//! → objects (ch. 20) → the model. Objects decode independently (in parallel on native builds); the model
//! is assembled in the same order as the JavaScript reader, so the output matches it field for field.

use std::collections::HashMap;

use crate::util::{IntMap, IntSet};

use crate::bits::{BitReader, Codepage, DwgError, Res};
use crate::container::{Container, SENTINEL_CLASSES, SENTINEL_HEADER, read_container, u16_at, u32_at};
use crate::crc::crc8;
use crate::header_vars::decode_header_vars;
use crate::messages::msg;
use crate::model::{lineweight_from_index, new_model, transparency_from_word};
use crate::obj;
use crate::objects::{Class, Ctx, Record, decode_object, string_stream_start};
use crate::util::{handle_hex as hx, handle_num, to_i64};
use crate::value::{Obj, V};

#[derive(Clone, Debug, Default)]
pub struct ReadOpts {
    /// Throw (return the first error as a note-only model) instead of reading on; mirrors `strict`.
    pub strict: bool,
}

/// The decoded records in object-map order (a JS `Map` keyed by handle).
#[derive(Debug, Default)]
pub struct Records {
    pub idx: IntMap<i64, usize>,
    pub list: Vec<Record>,
}
impl Records {
    pub fn get(&self, h: f64) -> Option<&Record> {
        if h == 0.0 || h.is_nan() {
            return None;
        }
        self.idx.get(&to_i64(h)).and_then(|&i| self.list.get(i))
    }
    pub fn get_mut(&mut self, h: f64) -> Option<&mut Record> {
        if h == 0.0 || h.is_nan() {
            return None;
        }
        let i = *self.idx.get(&to_i64(h))?;
        self.list.get_mut(i)
    }
    pub fn len(&self) -> usize {
        self.list.len()
    }
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = &Record> {
        self.list.iter()
    }
}

/// What the model-assembly helpers of the other modules read (read.js passes hx / nameOf / get / appName).
pub struct Build<'a> {
    pub ver: u32,
    pub ctx: &'a Ctx,
    /// the AcDb:AcDbObjects data the records were decoded from (for re-reads)
    pub buf: &'a [u8],
    pub records: &'a Records,
}
impl Build<'_> {
    pub fn get(&self, h: f64) -> Option<&Record> {
        self.records.get(h)
    }
    /// `nameOf(h)`: the record's `name` field when it has one.
    pub fn name_of(&self, h: f64) -> Option<V> {
        self.get(h).and_then(|r| r.f.get("name").cloned())
    }
    pub fn name_str(&self, h: f64) -> Option<String> {
        self.name_of(h).map(|v| js_string(&v))
    }
    /// `appName(h)`: nameOf ?? hx.
    pub fn app_name(&self, h: f64) -> String {
        self.name_str(h).unwrap_or_else(|| hx(h))
    }
}

/// JS `String(v)` for the values names can be.
pub fn js_string(v: &V) -> String {
    match v {
        V::Str(s) => s.clone(),
        V::Num(n) => {
            let mut s = String::new();
            crate::value::js_num(*n, &mut s);
            s
        }
        V::Bool(b) => b.to_string(),
        V::Null => "null".into(),
        V::Arr(_) | V::Obj(_) | V::Bytes(_) => String::new(),
    }
}

fn plain_err(e: &DwgError) -> bool {
    let m = &e.0;
    m.starts_with("implausible size") || m.starts_with("LZ ") || m.starts_with("R2007 LZ")
}

/// readDwg(bytes, opts) → the model.
pub fn read_dwg(bytes: &[u8], opts: &ReadOpts) -> Obj {
    let t0 = std::time::Instant::now();
    let mut model = new_model();
    let phases = std::env::var_os("CCDWG_PHASES").is_some();
    let mut lap = std::time::Instant::now();
    let mut phase = |name: &str| {
        if phases {
            eprintln!("ccdwg phase {name}: {:.2} ms", lap.elapsed().as_secs_f64() * 1000.0);
            lap = std::time::Instant::now();
        }
    };
    let c: Container = match read_container(bytes) {
        Ok(c) => c,
        Err(e) => {
            let v: String = bytes.iter().take(6).map(|&b| if (0x20..=0x7e).contains(&b) { char::from(b) } else { '?' }).collect();
            model.set("version", v);
            let note = if plain_err(&e) { msg("ccdwg.open.failed", &[("reason", &e.0)]) } else { e.0.clone() };
            if let Some(rep) = model.obj_mut("report") {
                rep.set("error", e.0);
                push_note(rep, note);
            }
            return model;
        }
    };
    let mut notes: Vec<String> = Vec::new();
    model.set("version", c.version.clone());
    model.set("maintenance", c.maintenance);
    notes.extend(c.notes.iter().cloned());
    let cp = Codepage::new(c.codepage_num);
    model.set("codepage", cp.name);
    if !cp.known {
        notes.push(msg("ccdwg.read.codepage", &[("num", &c.codepage_num.to_string()), ("name", cp.name)]));
    }
    let ver = c.ver;
    let mut ctx = Ctx { ver, maint: c.maintenance, cp, classes: HashMap::new(), strict: opts.strict };

    // ---- header (ch. 9)
    let mut header = match read_header_section(c.section("AcDb:Header"), &ctx, &mut notes) {
        Ok(h) => h,
        Err(e) => {
            notes.push(msg("ccdwg.read.header_failed", &[("reason", &e.0)]));
            Obj::new()
        }
    };
    header.set("ACADVER", c.version.clone());
    header.set("ACADMAINTVER", c.maintenance);
    header.set("DWGCODEPAGE", cp.name);
    if let Some(v) = header.get("OBSCUREDCOLOR").cloned() {
        header.set("OBSCOLOR", v);
    }
    if let Some(v) = header.get("OBSCUREDLTYPE").cloned() {
        header.set("OBSLTYPE", v);
    }
    if let Some(f) = header.num("FLAGS") {
        header.set("CELWEIGHT", lineweight_from_index(i64::from(crate::util::to_i32(f) & 0x1F)));
    }
    if let Some(tpl) = c.section("AcDb:Template")
        && tpl.len() >= 4
    {
        let n = crate::util::usize_from_u32(u16_at(tpl, 0)).saturating_mul(if ver >= 21 { 2 } else { 1 });
        if n.saturating_add(4) <= tpl.len() {
            header.set("MEASUREMENT", u16_at(tpl, n.saturating_add(2)));
            if n > 0 {
                let body = tpl.get(2..n.saturating_add(2)).unwrap_or_default();
                let d = if ver >= 21 {
                    let units: Vec<u16> = body
                        .chunks(2)
                        .map(|p| u16::from_le_bytes([p.first().copied().unwrap_or(0), p.get(1).copied().unwrap_or(0)]))
                        .collect();
                    let s = String::from_utf16_lossy(&units);
                    s.trim_end_matches('\0').to_string()
                } else {
                    cp.decode(body)
                };
                header.set("TEMPLATE_DESCRIPTION", d);
            }
        }
    }

    // ---- classes (ch. 10)
    phase("container+header");
    match read_classes_section(c.section("AcDb:Classes"), &ctx, &mut notes) {
        Ok(list) => {
            let mut arr = Vec::with_capacity(list.len());
            for (k, o) in list {
                ctx.classes.insert(k.num, k);
                arr.push(V::Obj(o));
            }
            model.set("classes", arr);
        }
        Err(e) => notes.push(msg("ccdwg.read.classes_failed", &[("reason", &e.0)])),
    }

    // ---- object map (ch. 23) and objects (ch. 20)
    let map = match read_object_map(c.section("AcDb:Handles"), &mut notes) {
        Ok(m) => m,
        Err(e) => {
            notes.push(msg("ccdwg.read.map_failed", &[("reason", &e.0)]));
            Vec::new()
        }
    };
    let empty: &[u8] = &[];
    let objs = c.section("AcDb:AcDbObjects");
    let buf = objs.unwrap_or(empty);
    let mut records = Records::default();
    let mut damaged = 0usize;
    let mut crc_bad = 0usize;
    let mut damaged_types: Vec<(String, usize)> = Vec::new();
    if objs.is_some() {
        phase("classes+map");
        let decoded = decode_all(buf, &map, &ctx);
        records.list.reserve(map.len());
        records.idx.reserve(map.len());
        phase("decode");
        for ((handle, _), res) in map.iter().zip(decoded) {
            let rec = match res {
                Ok(r) => r,
                Err(e) => {
                    damaged = damaged.saturating_add(1);
                    if damaged <= 20 {
                        notes.push(msg("ccdwg.read.object_failed", &[("handle", &hx(*handle)), ("reason", &e.0)]));
                    }
                    continue;
                }
            };
            if rec.handle != *handle {
                damaged = damaged.saturating_add(1);
                if damaged <= 20 {
                    notes.push(msg("ccdwg.read.map_entry", &[("handle", &hx(*handle)), ("other", &hx(rec.handle))]));
                }
                continue;
            }
            if rec.crc_bad {
                crc_bad = crc_bad.saturating_add(1);
            }
            if rec.error.is_some() {
                match damaged_types.iter_mut().find(|(t, _)| *t == rec.typ) {
                    Some(x) => x.1 = x.1.saturating_add(1),
                    None => damaged_types.push((rec.typ.clone(), 1)),
                }
            }
            let key = to_i64(*handle);
            match records.idx.get(&key) {
                Some(&i) => {
                    if let Some(slot) = records.list.get_mut(i) {
                        *slot = rec;
                    }
                }
                None => {
                    records.idx.insert(key, records.list.len());
                    records.list.push(rec);
                }
            }
        }
    } else {
        notes.push(msg("ccdwg.read.no_objects", &[]));
    }
    if damaged > 20 {
        notes.push(msg("ccdwg.read.more_failed", &[("n", &damaged.saturating_sub(20).to_string())]));
    }
    if crc_bad > 0 {
        notes.push(msg("ccdwg.read.crc_failed", &[("n", &crc_bad.to_string())]));
    }
    for (t, n) in &damaged_types {
        notes.push(msg("ccdwg.read.partial_type", &[("n", &n.to_string()), ("type", t)]));
    }
    phase("records-insert");
    if ver >= 27 {
        crate::solid_read::attach_data_storage(&mut records, c.section("AcDb:AcDsPrototype_1b"), &mut notes);
    }

    model.set("header", header);
    if let Some(rep) = model.obj_mut("report") {
        rep.set("notes", notes.into_iter().map(V::Str).collect::<Vec<V>>());
    }
    phase("records");
    build_model(&mut model, &mut records, &ctx, buf);
    phase("build");
    let _ = crate::dynblock_read::read_dynamic(&mut model);
    phase("dynamic");
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    if let Some(rep) = model.obj_mut("report") {
        rep.set("ms", ms.round());
        rep.set("objects", records.len());
    }
    // Freeing tens of thousands of records costs milliseconds the caller need not wait for.
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::spawn(move || drop(records));
    model
}

fn push_note(rep: &mut Obj, note: String) {
    match rep.get_mut("notes") {
        Some(V::Arr(a)) => a.push(V::Str(note)),
        _ => rep.set("notes", vec![V::Str(note)]),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn decode_all(buf: &[u8], map: &[(f64, f64)], ctx: &Ctx) -> Vec<Res<Record>> {
    use rayon::prelude::*;
    map.par_iter().map(|&(_, off)| decode_guarded(buf, off, ctx)).collect()
}
#[cfg(target_arch = "wasm32")]
fn decode_all(buf: &[u8], map: &[(f64, f64)], ctx: &Ctx) -> Vec<Res<Record>> {
    map.iter().map(|&(_, off)| decode_guarded(buf, off, ctx)).collect()
}

/// Last-resort guard: a panic in one object's decoder costs that object, not the read.
fn decode_guarded(buf: &[u8], off: f64, ctx: &Ctx) -> Res<Record> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| decode_object(buf, off, ctx))) {
            Ok(r) => r,
            Err(_) => Err(DwgError("internal error while decoding (please report this file)".into())),
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        decode_object(buf, off, ctx)
    }
}

// ---------------------------------------------------------------- ch. 9 header section
fn section_start(sec: &[u8], ctx: &Ctx) -> (usize, usize) {
    let size = crate::util::usize_from_u32(u32_at(sec, 16));
    let mut p = 20usize;
    if ctx.ver >= 24 && (ctx.maint > 3 || ctx.ver >= 32) {
        p = p.saturating_add(4);
    }
    (p, sec.len().min(p.saturating_add(size)))
}

fn read_header_section(sec: Option<&[u8]>, ctx: &Ctx, notes: &mut Vec<String>) -> Res<Obj> {
    let Some(sec) = sec else { return Err(DwgError(msg("ccdwg.err.no_header", &[]))) };
    if sec.get(..16) != Some(&SENTINEL_HEADER[..]) {
        notes.push(msg("ccdwg.read.header_sentinel", &[]));
    }
    let (p, data_end) = section_start(sec, ctx);
    if u32::from(crc8(0xC0C1, sec, 16, data_end)) != u16_at(sec, data_end) {
        notes.push(msg("ccdwg.read.header_crc", &[]));
    }
    let ver = ctx.ver;
    if ver < 21 {
        let mut r = BitReader::range(sec, ver, ctx.cp, p.saturating_mul(8), data_end.saturating_mul(8));
        let (vars, error) = decode_header_vars(&mut r, ver, None);
        if let Some(e) = error {
            notes.push(msg("ccdwg.read.header_early", &[("reason", &e)]));
        }
        return Ok(vars);
    }
    let bits = crate::util::usize_from_u32(u32_at(sec, p));
    let endbit = p.saturating_mul(8).saturating_add(bits);
    let start = p.saturating_add(4).saturating_mul(8);
    let mut r = BitReader::range(sec, ver, ctx.cp, start, endbit);
    match string_stream_start(sec, endbit, start) {
        Some((s, e)) => {
            r.set_str(s, e);
            r.end = s;
        }
        None => r.set_str(0, 0),
    }
    let mut hr = BitReader::range(sec, ver, ctx.cp, endbit, data_end.saturating_mul(8));
    let (vars, error) = decode_header_vars(&mut r, ver, Some(&mut hr));
    if let Some(e) = error {
        notes.push(msg("ccdwg.read.header_early", &[("reason", &e)]));
    }
    Ok(vars)
}

// ---------------------------------------------------------------- ch. 10 classes
fn read_classes_section(sec: Option<&[u8]>, ctx: &Ctx, notes: &mut Vec<String>) -> Res<Vec<(Class, Obj)>> {
    let Some(sec) = sec else { return Err(DwgError(msg("ccdwg.err.no_classes", &[]))) };
    if sec.get(..16) != Some(&SENTINEL_CLASSES[..]) {
        notes.push(msg("ccdwg.read.classes_sentinel", &[]));
    }
    let (p, data_end) = section_start(sec, ctx);
    if u32::from(crc8(0xC0C1, sec, 16, data_end)) != u16_at(sec, data_end) {
        notes.push(msg("ccdwg.read.classes_crc", &[]));
    }
    let ver = ctx.ver;
    let mut r = if ver >= 21 {
        let bits = crate::util::usize_from_u32(u32_at(sec, p));
        let endbit = p.saturating_mul(8).saturating_add(bits);
        let start = p.saturating_add(4).saturating_mul(8);
        let mut r = BitReader::range(sec, ver, ctx.cp, start, endbit);
        if let Some((s, e)) = string_stream_start(sec, endbit, start) {
            r.set_str(s, e);
            r.end = s;
        } else {
            r.set_str(0, sec.len().saturating_mul(8));
        }
        r
    } else {
        BitReader::range(sec, ver, ctx.cp, p.saturating_mul(8), data_end.saturating_mul(8))
    };
    let mut max_num = i32::MAX;
    if ver >= 18 {
        max_num = r.bs()?;
        r.rc()?;
        r.rc()?;
        r.b()?;
    }
    let mut out = Vec::new();
    while r.bits_left() > 40 {
        let num = r.bs()?;
        let proxy_flags = r.bs()?;
        let app_name = r.tv()?;
        let cpp_name = r.tv()?;
        let dxf_name = r.tv()?;
        let was_zombie = r.b()? != 0;
        let item_class = r.bs()?;
        let is_entity = item_class == 0x1F2;
        let mut o = obj! {
            "num" => num, "proxyFlags" => proxy_flags, "appName" => app_name, "cppName" => cpp_name.clone(),
            "dxfName" => dxf_name.clone(), "wasZombie" => was_zombie, "isEntity" => is_entity,
        };
        if ver >= 18 {
            o.set("instanceCount", r.bl()?);
            o.set("dwgVersion", r.bs()?);
            o.set("maintVersion", r.bs()?);
            r.bl()?;
            r.bl()?;
        }
        if !(500..=5000).contains(&num) {
            notes.push(msg("ccdwg.read.class_number", &[("num", &num.to_string())]));
            break;
        }
        out.push((Class { num, dxf_name, cpp_name, is_entity }, o));
        if num >= max_num {
            break;
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- ch. 23 object map
/// (handle, offset) pairs in map order; a handle seen again keeps its first position with the new offset.
fn read_object_map(sec: Option<&[u8]>, notes: &mut Vec<String>) -> Res<Vec<(f64, f64)>> {
    let Some(sec) = sec else { return Err(DwgError(msg("ccdwg.err.no_handles", &[]))) };
    let mut list: Vec<(f64, f64)> = Vec::new();
    let mut pos_of: IntMap<i64, usize> = IntMap::default();
    let mut p = 0usize;
    let mut bad = 0usize;
    while p.saturating_add(2) <= sec.len() {
        let size = (crate::util::usize_from_u32(u32::from(sec.get(p).copied().unwrap_or(0))) << 8)
            | usize::from(sec.get(p.saturating_add(1)).copied().unwrap_or(0));
        if size <= 2 {
            break;
        }
        let end = p.saturating_add(size);
        if end.saturating_add(2) > sec.len() {
            notes.push(msg("ccdwg.read.map_short", &[]));
            break;
        }
        let crc = (u16::from(sec.get(end).copied().unwrap_or(0)) << 8) | u16::from(sec.get(end.saturating_add(1)).copied().unwrap_or(0));
        if crc8(0xC0C1, sec, p, end) != crc {
            bad = bad.saturating_add(1);
        }
        let mut q = p.saturating_add(2);
        let mut h = 0.0f64;
        let mut loc = 0.0f64;
        while q < end {
            let (dh, q1) = crate::bits::read_mcu(sec, q);
            let (dl, q2) = crate::bits::read_mc(sec, q1);
            q = q2;
            h += dh;
            loc += dl;
            let key = to_i64(h);
            match pos_of.get(&key) {
                Some(&i) => {
                    if let Some(x) = list.get_mut(i) {
                        x.1 = loc;
                    }
                }
                None => {
                    pos_of.insert(key, list.len());
                    list.push((h, loc));
                }
            }
        }
        p = end.saturating_add(2);
    }
    if bad > 0 {
        notes.push(msg("ccdwg.read.map_crc", &[("n", &bad.to_string())]));
    }
    Ok(list)
}

// ---------------------------------------------------------------- model assembly

const SKIP_KEYS: [&str; 8] = ["handle", "typeNum", "type", "isEntity", "className", "color", "ltscale", "invisible"];
const SUB_TYPES: [&str; 9] =
    ["VERTEX_2D", "VERTEX_3D", "VERTEX_MESH", "VERTEX_PFACE", "VERTEX_PFACE_FACE", "SEQEND", "ATTRIB", "BLOCK", "ENDBLK"];

fn table_of(t: &str) -> Option<&'static str> {
    Some(match t {
        "LAYER" => "layers",
        "LTYPE" => "linetypes",
        "STYLE" => "styles",
        "DIMSTYLE" => "dimstyles",
        "VIEW" => "views",
        "UCS" => "ucs",
        "VPORT" => "vports",
        "APPID" => "appids",
        "BLOCK_HEADER" => "blockRecords",
        _ => return None,
    })
}
fn dim_base(t: &str) -> Option<i32> {
    Some(match t {
        "DIMENSION_LINEAR" => 0,
        "DIMENSION_ALIGNED" => 1,
        "DIMENSION_ANG2LN" => 2,
        "DIMENSION_DIAMETER" => 3,
        "DIMENSION_RADIUS" => 4,
        "DIMENSION_ANG3PT" => 5,
        "DIMENSION_ORDINATE" => 6,
        _ => return None,
    })
}

/// Copy the public fields of a record (no _private, no handle-ref bag); bytes become base64 text.
pub fn plain(rec: &Record) -> Obj {
    let mut o = Obj::with_capacity(rec.f.len());
    for (k, v) in &rec.f.0 {
        if k.starts_with('_') || SKIP_KEYS.contains(&k.as_ref()) {
            continue;
        }
        let v = match v {
            V::Bytes(b) => V::Str(crate::util::b64(b)),
            other => other.clone(),
        };
        o.0.push((k.clone(), v));
    }
    o
}

fn fnum(r: &Record, k: &str) -> f64 {
    r.f.num(k).unwrap_or(0.0)
}
fn truthy(r: &Record, k: &str) -> bool {
    r.f.truthy(k)
}
fn get_f(r: &Record, k: &str) -> Option<V> {
    r.f.get(k).cloned()
}
fn hlist(v: &[f64]) -> V {
    V::Arr(v.iter().map(|&h| V::Str(hx(h))).collect())
}
fn count_type_ref(e: &Obj) -> &str {
    if e.str("type") == Some("UNKNOWN") { e.str("dxfName").unwrap_or("") } else { e.str("type").unwrap_or("") }
}
fn inc(o: &mut Obj, k: &str) {
    match o.get_mut(k) {
        Some(V::Num(n)) => *n += 1.0,
        Some(other) => *other = V::Num(other.as_f64().unwrap_or(f64::NAN) + 1.0),
        None => o.0.push((std::borrow::Cow::Owned(k.to_string()), V::Num(1.0))),
    }
}
fn xdata_items(items: &[(i32, V)]) -> Vec<V> {
    items.iter().map(|(c, v)| V::Arr(vec![V::from(*c), v.clone()])).collect()
}
/// `data` of an XRECORD as (code, value) pairs.
fn xrec_data(r: &Record) -> Option<Vec<(f64, V)>> {
    let a = r.f.arr("data")?;
    Some(
        a.iter()
            .map(|g| match g {
                V::Arr(p) => (p.first().and_then(V::as_f64).unwrap_or(f64::NAN), p.get(1).cloned().unwrap_or(V::Null)),
                _ => (f64::NAN, V::Null),
            })
            .collect(),
    )
}
fn dict_names(r: &Record) -> Option<Vec<String>> {
    r.f.arr("names").map(|a| a.iter().map(js_string).collect())
}

fn block_flags(r: &Record) -> i32 {
    (if truthy(r, "anonymous") { 1 } else { 0 })
        | (if truthy(r, "hasAttribs") { 2 } else { 0 })
        | (if truthy(r, "isXref") { 4 } else { 0 })
        | (if truthy(r, "isOverlaid") { 8 } else { 0 })
        | (if truthy(r, "_xdep") { 16 } else { 0 })
        | (if truthy(r, "_flag64") { 64 } else { 0 })
}
fn dep_flags(r: &Record) -> i32 {
    (if truthy(r, "_xdep") { 16 } else { 0 }) | (if truthy(r, "_flag64") { 64 } else { 0 })
}

fn build_model(model: &mut Obj, records: &mut Records, ctx: &Ctx, buf: &[u8]) {
    let mut build_errors = 0usize;
    let mut notes: Vec<String> = Vec::new();
    if let Err(e) = build_model_parts(model, records, ctx, buf, &mut build_errors, &mut notes) {
        notes.push(msg("ccdwg.read.assembled_part", &[("reason", &e.0)]));
    }
    if build_errors > 0 {
        notes.push(msg("ccdwg.read.kept_common", &[("n", &build_errors.to_string())]));
    }
    if let Some(rep) = model.obj_mut("report") {
        for n in notes {
            push_note(rep, n);
        }
    }
}

/// One `fix_records` edit: (handle, rename (old, new), lineweight, plot flag).
type RecordEdit = (f64, Option<(V, V)>, Option<f64>, Option<bool>);

/// First pass: the record changes read.js makes before converting (long names, true names, anonymous numbers).
fn fix_records(model: &mut Obj, records: &mut Records, ver: u32) {
    let hnum = |model: &Obj, k: &str| model.obj("header").and_then(|h| h.str(k)).map(handle_num).unwrap_or(0.0);
    // ACAD_XREC_ROUNDTRIP long names, R13/R14 layer lineweight + plot bit
    let mut edits: Vec<RecordEdit> = Vec::new();
    for r in records.iter() {
        if table_of(&r.typ).is_none() {
            continue;
        }
        let Some(xdic) = r.hn("xdic") else { continue };
        let Some(d) = records.get(xdic) else { continue };
        if d.typ != "DICTIONARY" {
            continue;
        }
        let Some(names) = dict_names(d) else { continue };
        let Some(k) = names.iter().position(|n| n == "ACAD_XREC_ROUNDTRIP") else { continue };
        let items = d.hlist("items").unwrap_or_default();
        let Some(x) = items.get(k).and_then(|&h| records.get(h)) else { continue };
        if x.typ != "XRECORD" {
            continue;
        }
        let Some(data) = xrec_data(x) else { continue };
        let mut rename = None;
        if let Some(i) = data.iter().position(|g| g.0 == 102.0 && g.1.as_str() == Some("EXTNAMES"))
            && let (Some(g2), Some(g1)) = (data.get(i.saturating_add(2)), data.get(i.saturating_add(1)))
            && g2.0 == 2.0
            && Some(&g1.1) == r.f.get("name")
        {
            rename = Some((r.f.get("name").cloned().unwrap_or(V::Null), g2.1.clone()));
        }
        let mut lw = None;
        let mut plot = None;
        if r.typ == "LAYER" && ver < 15 {
            if let Some(i) = data.iter().position(|g| g.0 == 102.0 && g.1.as_str() == Some("LINEWEIGHT"))
                && let Some(g) = data.get(i.saturating_add(1))
            {
                lw = Some(f64::from(crate::util::to_i32(g.1.as_f64().unwrap_or(0.0)) & 0x1F));
            }
            if let Some(i) = data.iter().position(|g| g.0 == 102.0 && g.1.as_str() == Some("PLOTBIT"))
                && let Some(g) = data.get(i.saturating_add(1))
            {
                plot = Some(g.1.truthy());
            }
        }
        if rename.is_some() || lw.is_some() || plot.is_some() {
            edits.push((r.handle, rename, lw, plot));
        }
    }
    for (h, rename, lw, plot) in edits {
        if let Some(r) = records.get_mut(h) {
            if let Some((short, long)) = rename {
                r.f.set("_shortName", short);
                r.f.set("name", long);
            }
            if let Some(lw) = lw {
                r.f.set("_lwIndex", lw);
            }
            if let Some(p) = plot {
                r.f.set("plot", p);
            }
        }
    }
    // R13/R14 header round trip
    if ver < 15 {
        let nod = records.get(hnum(model, "DICTIONARY_NAMED_OBJECTS"));
        let mut adds: Vec<(String, V)> = Vec::new();
        if let Some(nod) = nod
            && let Some(names) = dict_names(nod)
            && let Some(k) = names.iter().position(|n| n == "ACDBHEADERROUNDTRIPXREC")
            && let Some(d) = nod.hlist("items").unwrap_or_default().get(k).and_then(|&h| records.get(h))
            && let Some(dn) = dict_names(d)
        {
            let items = d.hlist("items").unwrap_or_default();
            for (j, name) in dn.iter().enumerate() {
                let Some(x) = items.get(j).and_then(|&h| records.get(h)) else { continue };
                if x.typ != "XRECORD" {
                    continue;
                }
                let Some(data) = xrec_data(x) else { continue };
                if data.len() != 1 {
                    continue;
                }
                if let Some(g) = data.first() {
                    adds.push((name.clone(), g.1.clone()));
                }
            }
        }
        if let Some(hd) = model.obj_mut("header") {
            for (name, v) in adds {
                if !hd.has(&name) {
                    hd.set(name, v);
                }
            }
        }
    }
    // dynamic block true names
    let owns_graph = |records: &Records, r: &Record| -> bool {
        r.hn("xdic").and_then(|x| records.get(x)).and_then(dict_names).is_some_and(|n| n.iter().any(|s| s == "ACAD_ENHANCEDBLOCK"))
    };
    let mut used_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for r in records.iter() {
        if r.typ == "BLOCK_HEADER"
            && let Some(n) = r.f.get("name")
            && n.truthy()
        {
            let s = js_string(n);
            if !s.starts_with('*') {
                used_names.insert(s.to_uppercase());
            }
        }
    }
    let mut renames: Vec<(usize, Option<V>, V, bool)> = Vec::new();
    for (i, r) in records.list.iter().enumerate() {
        let name = r.f.get("name").map(js_string).unwrap_or_default();
        if r.typ != "BLOCK_HEADER" || r.eed.is_none() || !(name.len() >= 2 && name.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("*U"))) {
            continue;
        }
        let mut cur_name = r.f.get("name").cloned();
        for x in r.eed.iter().flatten() {
            let app = records.get(x.app).and_then(|a| a.f.get("name").map(js_string)).unwrap_or_else(|| hx(x.app));
            if (app == "AcDbDynamicBlockTrueName2" || app == "AcDbDynamicBlockTrueName")
                && let Some((1000, tn)) = x.items.first()
            {
                let tns = js_string(tn);
                let rename = app == "AcDbDynamicBlockTrueName2"
                    || (owns_graph(records, r) && tn.truthy() && !used_names.contains(&tns.to_uppercase()));
                if rename {
                    used_names.insert(tns.to_uppercase());
                    renames.push((i, cur_name.clone(), tn.clone(), true));
                    cur_name = Some(tn.clone());
                } else {
                    renames.push((i, None, tn.clone(), false));
                }
            }
        }
    }
    for (i, stored, tn, rename) in renames {
        if let Some(r) = records.list.get_mut(i) {
            r.f.set("_trueName", tn.clone());
            if rename {
                r.f.set_opt("_storedName", stored);
                r.f.set("name", tn);
            }
        }
    }
    // anonymous block numbering
    let ctl = records.get(hnum(model, "BLOCK_CONTROL_OBJECT")).and_then(|c| c.hlist("entries")).unwrap_or_default();
    let ps_h = hnum(model, "BLOCK_RECORD_PAPER_SPACE");
    let ms_h = hnum(model, "BLOCK_RECORD_MODEL_SPACE");
    let mut i: i64 = -1;
    for h in ctl {
        if h == 0.0 {
            continue;
        }
        i = i.saturating_add(1);
        let Some(r) = records.get_mut(h) else { continue };
        if r.typ != "BLOCK_HEADER" || h == ps_h || h == ms_h {
            continue;
        }
        let Some(V::Str(name)) = r.f.get("name").cloned() else { continue };
        let single = name.len() == 2 && name.starts_with('*') && name.chars().nth(1).is_some_and(|c| c.is_ascii_alphabetic());
        if single || name.eq_ignore_ascii_case("*Paper_Space") {
            r.f.set("_storedName", name.clone());
            r.f.set("name", format!("{name}{i}"));
        }
    }
}

/// What converting one entity changes besides its output (kept per thread, merged in order).
#[derive(Default)]
struct Local {
    used: Vec<i64>,
    unknown: Vec<String>,
    build_errors: usize,
}

struct Conv<'a> {
    b: Build<'a>,
    ver: u32,
    ms_rec: f64,
    ps_rec: f64,
    used: IntSet<i64>,
    unknown: Obj,
    build_errors: usize,
    mlatt_fold: IntSet<i64>,
}

/// Convert a block's entities, in parallel on native builds; results come back in input order.
fn convert_many(cv: &Conv, todo: &[(f64, f64)]) -> Vec<(Obj, Local)> {
    let one = |&(h, owner): &(f64, f64)| {
        let mut l = Local::default();
        let e = cv.convert(&mut l, h, owner);
        (e, l)
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        if todo.len() > 256 {
            return todo.par_iter().with_min_len(64).map(one).collect();
        }
    }
    todo.iter().map(one).collect()
}

impl<'a> Conv<'a> {
    fn merge(&mut self, l: Local) {
        self.used.extend(l.used);
        for t in l.unknown {
            inc(&mut self.unknown, &t);
        }
        self.build_errors = self.build_errors.saturating_add(l.build_errors);
    }
    fn get(&self, h: f64) -> Option<&'a Record> {
        let recs: &'a Records = self.b.records;
        recs.get(h)
    }
    fn name_or_hx(&self, h: f64) -> V {
        self.b.name_of(h).unwrap_or_else(|| V::Str(hx(h)))
    }
    fn app_name(&self, h: f64) -> String {
        self.b.app_name(h)
    }
    fn xdata_of(&self, rec: &Record) -> Option<Obj> {
        let eed = rec.eed.as_ref()?;
        let mut x = Obj::new();
        for e in eed {
            let k = self.app_name(e.app);
            let items = xdata_items(&e.items);
            match x.get_mut(&k) {
                Some(V::Arr(a)) => a.extend(items),
                _ => x.set(k, items),
            }
        }
        Some(x)
    }
    fn common_obj(&self, rec: &Record) -> Obj {
        let mut o = obj! { "handle" => hx(rec.handle) };
        if let Some(ow) = rec.hn("owner")
            && ow != 0.0
        {
            o.set("owner", hx(ow));
        }
        if let Some(xd) = self.xdata_of(rec) {
            o.set("xdata", xd);
        }
        if let Some(x) = rec.hn("xdic")
            && x != 0.0
        {
            o.set("extDict", hx(x));
        }
        if let Some(r) = rec.hlist("reactors")
            && !r.is_empty()
        {
            o.set("reactors", hlist(&r));
        }
        o
    }
    fn owner_of(&self, rec: &Record) -> f64 {
        if let Some(o) = rec.hn("owner")
            && o != 0.0
        {
            return o;
        }
        match rec.entmode {
            2 => self.ms_rec,
            1 => self.ps_rec,
            _ => 0.0,
        }
    }
    fn layer_name(&self, h: f64) -> V {
        match self.get(h) {
            Some(r) => r.f.get("name").cloned().unwrap_or(V::Null),
            None => V::from("0"),
        }
    }
    /// R13–R2000 entity chains.
    fn chain(&self, first: f64, last: f64) -> Vec<f64> {
        let mut out = Vec::new();
        let mut seen = IntSet::default();
        let mut h = first;
        while h != 0.0 && !seen.contains(&to_i64(h)) && out.len() <= self.b.records.len() {
            seen.insert(to_i64(h));
            let r = self.get(h);
            out.push(h);
            let Some(r) = r else { break };
            if h == last {
                break;
            }
            let next = if r.nolinks != 0 { h + 1.0 } else { r.hn("next").unwrap_or(0.0) };
            if next == 0.0 || next == h {
                break;
            }
            h = next;
        }
        out
    }
    fn mlatt_of(&self, rec: &Record) -> Option<Vec<(f64, V)>> {
        let d = self.get(rec.hn("xdic")?)?;
        if d.typ != "DICTIONARY" {
            return None;
        }
        let names = dict_names(d)?;
        let k = names.iter().position(|n| n == "ACAD_MLATT")?;
        let x = d.hlist("items").unwrap_or_default().get(k).and_then(|&h| self.get(h))?;
        if x.typ != "XRECORD" {
            return None;
        }
        xrec_data(x)
    }
    fn apply_mlatt(&self, rec: &Record, f: &mut Obj) {
        let Some(data) = self.mlatt_of(rec) else { return };
        let g = |code: f64, nth: usize, d: &[(f64, V)]| d.iter().filter(|x| x.0 == code).nth(nth).map(|x| x.1.clone());
        let Some(ti) = data.iter().position(|x| x.0 == 2.0) else { return };
        let after = data.get(ti.saturating_add(1)..).unwrap_or_default();
        f.set_opt("attType", g(70.0, 0, &data));
        let tag = f.get("tag").cloned();
        f.set_opt("storedTag", tag);
        f.set("tag", data.get(ti).map(|x| x.1.clone()).unwrap_or(V::Null));
        let mut m = Obj::new();
        for (k, code, nth) in [
            ("insert", 10.0, 0),
            ("height", 40.0, 0),
            ("width", 41.0, 0),
            ("rectHeight", 46.0, 0),
            ("attach", 71.0, 0),
            ("direction", 72.0, 0),
            ("value", 1.0, 1),
            ("xAxis", 11.0, 0),
            ("lineSpacingStyle", 73.0, 0),
            ("lineSpacing", 44.0, 0),
        ] {
            m.set_opt(k, g(code, nth, after));
        }
        let mv = m.get("value").cloned();
        f.set("mtext", m);
        if let Some(v) = mv {
            let old = f.get("value").cloned();
            f.set_opt("storedValue", old);
            f.set("value", v);
        }
    }

    fn convert(&self, l: &mut Local, rec_h: f64, owner_handle: f64) -> Obj {
        let Some(rec) = self.get(rec_h) else { return Obj::new() };
        l.used.push(to_i64(rec.handle));
        let ver = self.ver;
        let owner = if owner_handle != 0.0 { owner_handle } else { self.owner_of(rec) };
        let mut e = obj! { "handle" => hx(rec.handle), "type" => rec.typ.clone(), "owner" => hx(owner) };
        e.set("layer", self.layer_name(rec.hn("layer").unwrap_or(0.0)));
        let lt = rec.hn("ltype").unwrap_or(0.0);
        let linetype = if ver < 15 {
            if rec.bylayerlt != 0 { V::from("ByLayer") } else { self.name_or_hx(lt) }
        } else {
            match rec.ltflags {
                Some(0) => V::from("ByLayer"),
                Some(1) => V::from("ByBlock"),
                Some(2) => V::from("Continuous"),
                _ => self.name_or_hx(lt),
            }
        };
        e.set("linetype", linetype);
        e.set("color", rec.color.clone().unwrap_or_else(|| obj! { "index" => 256 }));
        e.set(
            "lineweight",
            match rec.lw {
                Some(i) => lineweight_from_index(i64::from(i)),
                None => -1,
            },
        );
        e.set_opt("ltscale", rec.ltscale.map(V::Num));
        e.set_opt("invisible", rec.invisible.map(V::Bool));
        if let Some(t) = rec.transparency {
            e.set("transparency", transparency_from_word(t));
        }
        for (k, hk) in [("material", "material"), ("plotStyle", "plotstyle"), ("colorBook", "colorBook")] {
            if let Some(h) = rec.hn(hk)
                && h != 0.0
            {
                e.set(k, hx(h));
            }
        }
        let xd = self.xdata_of(rec);
        if let Some(xd) = &xd {
            e.set("xdata", xd.clone());
            if let Some(V::Arr(a)) = xd.get("ACAD")
                && let Some(k) = a.iter().position(|it| {
                    it.as_arr()
                        .is_some_and(|p| p.first().and_then(V::as_f64) == Some(1000.0) && p.get(1).and_then(V::as_str) == Some("RTLwPs"))
                })
                && let Some(V::Arr(p)) = a.get(k.saturating_add(1))
                && p.first().and_then(V::as_f64) == Some(1070.0)
            {
                let v = crate::util::to_i32(p.get(1).and_then(V::as_f64).unwrap_or(0.0));
                if v & 128 != 0 {
                    e.set("lineweight", lineweight_from_index(i64::from(v & 0x7F)));
                }
            }
        }
        if let Some(x) = rec.hn("xdic")
            && x != 0.0
        {
            e.set("extDict", hx(x));
        }
        if let Some(r) = rec.hlist("reactors")
            && !r.is_empty()
        {
            e.set("reactors", hlist(&r));
        }
        if rec.unknown {
            e.set("type", "UNKNOWN");
            e.set("typeNum", rec.type_num);
            e.set("dxfName", rec.typ.clone());
            e.set("className", rec.class_name.clone().unwrap_or_default());
            if let Some(raw) = crate::passthrough::capture_raw(rec, ver, self.b.buf) {
                e.set("raw", raw);
            }
            if let Some(de) = rec.data_end {
                e.set("bits", crate::util::usize_to_f64(de) - crate::util::usize_to_f64(rec.data_start));
            }
            l.unknown.push(rec.typ.clone());
            return e;
        }
        match self.type_fields(l, rec) {
            Ok(f) => e.assign(f),
            Err(err) => {
                let prev = rec.error.as_ref().map(|x| format!("{x}; ")).unwrap_or_default();
                e.set("damaged", format!("{prev}model: {}", err.0));
                l.build_errors = l.build_errors.saturating_add(1);
                return e;
            }
        }
        if let Some(er) = &rec.error {
            e.set("damaged", er.clone());
        }
        e
    }

    fn type_fields(&self, l: &mut Local, rec: &'a Record) -> Res<Obj> {
        let mut f = plain(rec);
        let ver = self.ver;
        let h = |k: &str| rec.hn(k).unwrap_or(0.0);
        match rec.typ.as_str() {
            "TEXT" | "ATTRIB" | "ATTDEF" => {
                f.set("style", self.name_or_hx(h("style")));
                if rec.typ != "TEXT" && h("xdic") != 0.0 {
                    self.apply_mlatt(rec, &mut f);
                }
            }
            "MTEXT" => {
                f.set("style", self.name_or_hx(h("style")));
                if ver < 15
                    && let Some(ax) = rec.eed.iter().flatten().find(|x| self.app_name(x.app) == "ACAD")
                {
                    let it = &ax.items;
                    if let Some(b0) = it.iter().position(|g| g.1.as_str() == Some("MTEXTBEGIN")) {
                        let mut k = b0.saturating_add(1);
                        while k.saturating_add(1) < it.len() && it.get(k).and_then(|g| g.1.as_str()) != Some("MTEXTEND") {
                            let (Some(a), Some(b)) = (it.get(k), it.get(k.saturating_add(1))) else { break };
                            if a.0 != 1070 {
                                break;
                            }
                            match a.1.as_f64() {
                                Some(73.0) => f.set("lineSpacingStyle", b.1.clone()),
                                Some(44.0) => f.set("lineSpacing", b.1.clone()),
                                Some(46.0) => f.set("rectHeight", b.1.clone()),
                                _ => {}
                            }
                            k = k.saturating_add(2);
                        }
                    }
                }
                if let Some(d) = self.get(h("xdic"))
                    && d.typ == "DICTIONARY"
                    && let Some(names) = dict_names(d)
                    && let Some(k) = names.iter().position(|n| n == "ACAD_MTEXT_RT")
                    && let Some(xr) = d.hlist("items").unwrap_or_default().get(k).and_then(|&x| self.get(x))
                    && xr.typ == "XRECORD"
                    && let Some(data) = xrec_data(xr)
                {
                    let t: String = data.iter().filter(|d| d.0 == 1.0).map(|d| js_string(&d.1)).collect();
                    if !t.is_empty() {
                        let old = f.get("value").cloned();
                        f.set_opt("storedValue", old);
                        f.set("value", t);
                    }
                }
            }
            "SHAPE" => f.set("style", self.name_or_hx(h("style"))),
            "INSERT" | "MINSERT" => {
                f.set("block", self.name_or_hx(h("block")));
                f.set("blockRecord", hx(h("block")));
                let mut ah = rec.hlist("attribs");
                if ah.is_none() && h("firstAttrib") != 0.0 {
                    ah = Some(self.chain(h("firstAttrib"), h("lastAttrib")));
                }
                let mut atts = Vec::new();
                for a in ah.unwrap_or_default() {
                    let is_attrib = self.get(a).is_some_and(|r| r.typ == "ATTRIB");
                    if is_attrib {
                        if self.mlatt_fold.contains(&to_i64(a)) {
                            l.used.push(to_i64(a));
                        } else {
                            atts.push(V::Obj(self.convert(l, a, rec.handle)));
                        }
                    }
                }
                f.set("attribs", atts);
                if h("seqend") != 0.0 {
                    l.used.push(to_i64(h("seqend")));
                }
            }
            "POLYLINE_2D" | "POLYLINE_3D" | "POLYLINE_PFACE" | "POLYLINE_MESH" => {
                let mut vh = rec.hlist("vertices");
                if vh.is_none() && h("firstVertex") != 0.0 {
                    vh = Some(self.chain(h("firstVertex"), h("lastVertex")));
                }
                let mut verts = Vec::new();
                let mut faces = Vec::new();
                for vhd in vh.unwrap_or_default() {
                    let Some(v) = self.get(vhd) else { continue };
                    if !v.typ.starts_with("VERTEX") {
                        continue;
                    }
                    l.used.push(to_i64(vhd));
                    if v.typ == "VERTEX_PFACE_FACE" {
                        if let Some(ix) = v.f.get("indices")
                            && ix.truthy()
                        {
                            faces.push(ix.clone());
                        }
                        continue;
                    }
                    let Some(pt) = v.f.obj("point") else { continue };
                    let mut o = Obj::new();
                    o.set_opt("x", pt.get("x").cloned());
                    o.set_opt("y", pt.get("y").cloned());
                    o.set_opt("z", pt.get("z").cloned());
                    o.set_opt("flags", get_f(v, "flags"));
                    if v.typ == "VERTEX_2D" {
                        o.set_opt("z", get_f(rec, "elevation"));
                        o.set_opt("startWidth", get_f(v, "startWidth"));
                        o.set_opt("endWidth", get_f(v, "endWidth"));
                        o.set_opt("bulge", get_f(v, "bulge"));
                        o.set_opt("tangent", get_f(v, "tangent"));
                        if let Some(id) = get_f(v, "id") {
                            o.set("id", id);
                        }
                    }
                    verts.push(V::Obj(o));
                }
                if h("seqend") != 0.0 {
                    l.used.push(to_i64(h("seqend")));
                }
                f.set("vertices", verts);
                if rec.typ == "POLYLINE_PFACE" {
                    f.set("faces", faces);
                }
                if rec.typ == "POLYLINE_3D" {
                    let cf = crate::util::to_i32(fnum(rec, "_closedFlags"));
                    let sf = crate::util::to_i32(fnum(rec, "_splineFlags"));
                    f.set("flags", 8 | (cf & 1) | (if sf & 3 != 0 { 4 } else { 0 }));
                    f.set(
                        "curveType",
                        if sf & 1 != 0 {
                            5
                        } else if sf & 2 != 0 {
                            6
                        } else {
                            0
                        },
                    );
                }
                if rec.typ == "POLYLINE_PFACE" {
                    f.set("flags", 64);
                }
                if let Some(fl) = f.get("flags").cloned() {
                    let n = fl.as_f64().unwrap_or(f64::NAN);
                    f.set("closed", crate::util::to_i32(n) & 1 != 0);
                }
            }
            "LEADER" => {
                f.set("annotation", hx(h("annotation")));
                f.set("style", self.name_or_hx(h("dimstyle")));
            }
            "TOLERANCE" => f.set("style", self.name_or_hx(h("dimstyle"))),
            "MLINE" => f.set("style", self.name_or_hx(h("style"))),
            "IMAGE" | "WIPEOUT" => {
                f.set("def", hx(h("def")));
                f.set("defReactor", hx(h("defReactor")));
            }
            "VIEWPORT" => {
                if ver < 15 {
                    let ax = rec.eed.iter().flatten().find(|x| self.app_name(x.app) == "ACAD");
                    f.assign(mview_xdata(ax.map(|x| x.items.as_slice())));
                }
                f.set("frozenLayers", hlist(&rec.hlist("frozenLayers").unwrap_or_default()));
                if h("clipBoundary") != 0.0 {
                    f.set("clipBoundary", hx(h("clipBoundary")));
                }
            }
            "HATCH" => {
                let Some(V::Arr(loops)) = f.get_mut("loops") else {
                    return Err(DwgError("f.loops is not iterable".into()));
                };
                for l in loops.iter_mut() {
                    if let V::Obj(l) = l {
                        let b: Vec<f64> = l.arr("_boundary").map(|a| a.iter().filter_map(V::as_f64).collect()).unwrap_or_default();
                        l.set("boundary", hlist(&b));
                        l.remove("_boundary");
                        l.remove("_nbound");
                    }
                }
            }
            "SPLINE" => {
                let fit_n = f.arr("fit").map(Vec::len).unwrap_or(0);
                let knots_n = f.arr("knots").map(Vec::len).unwrap_or(0);
                let tol_ok = f.num("fitTolerance").is_none_or(|t| t.is_nan() || t <= 1e-9);
                if fnum(rec, "scenario") == 2.0 && fit_n >= 2 && knots_n == 0 && tol_ok {
                    let fit = f.arr("fit").cloned().unwrap_or_default();
                    let kp = f.num("knotParam").unwrap_or(0.0);
                    if let Some((knots, control)) =
                        fit_spline_control(&fit, f.num("degree").unwrap_or(0.0), f.obj("startTangent"), f.obj("endTangent"), kp)
                    {
                        f.set("knots", knots.into_iter().map(V::Num).collect::<Vec<V>>());
                        f.set("control", control);
                        f.set("controlComputed", true);
                    }
                }
            }
            "MULTILEADER" | "MLEADER" => {
                f.set("style", self.name_or_hx(h("style")));
                if let Some(ctxo) = rec.f.obj("context") {
                    if let Some(bl) = ctxo.obj("block") {
                        let bh = bl.num("_block").unwrap_or(0.0);
                        f.set("contentBlock", self.name_or_hx(bh));
                    }
                    if let Some(t) = ctxo.obj("text") {
                        let sh = t.num("_style").unwrap_or(0.0);
                        f.set_opt("textStyle", self.b.name_of(sh));
                    }
                }
                if let Ok(Some(m)) = crate::mleader_read::read_mleader(rec, &self.b) {
                    for k in ["context", "arrowheads", "blockLabels", "mlVersion"] {
                        f.remove(k);
                    }
                    f.assign(m);
                }
            }
            "ACAD_PROXY_ENTITY" => {
                if let Some(raw) = crate::passthrough::capture_raw(rec, ver, self.b.buf) {
                    f.set("raw", raw);
                }
                f.set("typeNum", rec.type_num);
            }
            "ACAD_TABLE" => crate::table_read::table_entity_fields(rec, &mut f, &self.b)?,
            "REGION" | "3DSOLID" | "BODY" => crate::solid_read::solid_fields(rec, &mut f, &self.b),
            "SURFACE" | "PLANESURFACE" | "EXTRUDEDSURFACE" | "LOFTEDSURFACE" | "REVOLVEDSURFACE" | "SWEPTSURFACE" | "NURBSURFACE" => {
                crate::surface_read::surface_fields(rec, &mut f, &self.b)?
            }
            t => {
                if dim_base(t).is_some() || t == "ARC_DIMENSION" || t == "LARGE_RADIAL_DIMENSION" {
                    f.set("block", self.name_or_hx(h("block")));
                    f.set("style", self.name_or_hx(h("dimstyle")));
                    let base = match dim_base(t) {
                        Some(b) => b,
                        None => crate::util::to_i32(f.num("dimType").unwrap_or(0.0)),
                    };
                    let f1 = crate::util::to_i32(fnum(rec, "_flags1"));
                    let f2 = crate::util::to_i32(fnum(rec, "_flags2"));
                    let flags = base
                        | (if f1 & 1 != 0 { 0 } else { 128 })
                        | (if f1 & 2 != 0 { 32 } else { 0 })
                        | (if t == "DIMENSION_ORDINATE" && f2 & 1 != 0 { 64 } else { 0 });
                    f.set("flags", flags);
                }
            }
        }
        Ok(f)
    }

    fn object_fields(&self, r: &Record) -> Res<Obj> {
        let mut f = plain(r);
        let h = |k: &str| r.hn(k).unwrap_or(0.0);
        let hl = |k: &str| hlist(&r.hlist(k).unwrap_or_default());
        match r.typ.as_str() {
            "DICTIONARY" | "ACDBDICTIONARYWDFLT" | "DICTIONARYWDFLT" => {
                let mut entries = Obj::new();
                let items = r.hlist("items").unwrap_or_default();
                for (i, n) in dict_names(r).unwrap_or_default().into_iter().enumerate() {
                    entries.set(n, hx(items.get(i).copied().unwrap_or(0.0)));
                }
                f.set("entries", entries);
                f.remove("names");
                if h("defaultEntry") != 0.0 {
                    f.set("defaultEntry", hx(h("defaultEntry")));
                }
            }
            "XRECORD" => f.set("objids", hl("objids")),
            "GROUP" => f.set("entities", hl("entities")),
            "LAYOUT" => {
                f.set("block", hx(h("block")));
                f.set("viewports", hl("viewports"));
            }
            "SORTENTSTABLE" => {
                f.set("ownerBlock", hx(h("ownerBlock")));
                f.set("entities", hl("ents"));
                let s: Vec<f64> = r.f.arr("_sort").map(|a| a.iter().filter_map(V::as_f64).collect()).unwrap_or_default();
                f.set("sortHandles", hlist(&s));
            }
            "IDBUFFER" => f.set("ids", hl("ids")),
            "FIELDLIST" => f.set("fields", hl("fields")),
            "TABLESTYLE" | "CELLSTYLEMAP" | "TABLECONTENT" | "TABLEGEOMETRY" | "FIELD" => {
                crate::table_read::table_object_fields(r, &mut f, &self.b)?
            }
            "LAYER_INDEX" => {
                let ents = r.hlist("entries").unwrap_or_default();
                let list: Vec<V> =
                    r.f.arr("entries")
                        .map(|a| {
                            a.iter()
                                .enumerate()
                                .map(|(i, e)| {
                                    let mut o = e.as_obj().cloned().unwrap_or_default();
                                    o.set("handle", hx(ents.get(i).copied().unwrap_or(0.0)));
                                    V::Obj(o)
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                f.set("entries", list);
            }
            "MLINESTYLE" => {
                if let Some(V::Arr(els)) = f.get_mut("elements") {
                    for e in els.iter_mut() {
                        if let V::Obj(e) = e
                            && let Some(lt) = e.remove("_ltype")
                        {
                            e.set("linetype", hx(lt.as_f64().unwrap_or(0.0)));
                        }
                    }
                }
            }
            "MLEADERSTYLE" => {
                for k in ["leaderLtype", "arrowhead", "textStyle", "block"] {
                    if let Some(v) = r.hn(k) {
                        f.set(k, hx(v));
                    }
                }
                if let Ok(Some(m)) = crate::mleader_read::read_mleader_style(r, &self.b) {
                    f.remove("leaderLtype");
                    f.assign(m);
                }
            }
            _ => {}
        }
        Ok(f)
    }
}

fn build_model_parts(
    model: &mut Obj,
    records: &mut Records,
    ctx: &Ctx,
    buf: &[u8],
    build_errors: &mut usize,
    notes: &mut Vec<String>,
) -> Res<()> {
    let phases = std::env::var_os("CCDWG_PHASES").is_some();
    let mut lap = std::time::Instant::now();
    let mut ph = |name: &str| {
        if phases {
            eprintln!("ccdwg   build {name}: {:.2} ms", lap.elapsed().as_secs_f64() * 1000.0);
            lap = std::time::Instant::now();
        }
    };
    let ver = ctx.ver;
    fix_records(model, records, ver);
    let records: &Records = records;
    ph("fix_records");
    let hnum = |k: &str| model.obj("header").and_then(|h| h.str(k)).map(handle_num).unwrap_or(0.0);
    let controls = [
        ("blockRecords", hnum("BLOCK_CONTROL_OBJECT")),
        ("layers", hnum("LAYER_CONTROL_OBJECT")),
        ("styles", hnum("STYLE_CONTROL_OBJECT")),
        ("linetypes", hnum("LINETYPE_CONTROL_OBJECT")),
        ("views", hnum("VIEW_CONTROL_OBJECT")),
        ("ucs", hnum("UCS_CONTROL_OBJECT")),
        ("vports", hnum("VPORT_CONTROL_OBJECT")),
        ("appids", hnum("APPID_CONTROL_OBJECT")),
        ("dimstyles", hnum("DIMSTYLE_CONTROL_OBJECT")),
    ];
    let ms_rec = hnum("BLOCK_RECORD_MODEL_SPACE");
    let ps_rec = hnum("BLOCK_RECORD_PAPER_SPACE");
    let mut table_recs: HashMap<&str, Vec<&Record>> = HashMap::new();
    for (t, ch) in controls {
        let mut seen = IntSet::default();
        let mut list = Vec::new();
        if let Some(c) = records.get(ch)
            && let Some(ents) = c.hlist("entries")
        {
            for h in ents {
                if let Some(r) = records.get(h)
                    && !seen.contains(&to_i64(h))
                    && table_of(&r.typ) == Some(t)
                {
                    seen.insert(to_i64(h));
                    list.push(r);
                }
            }
        }
        for r in records.iter() {
            if table_of(&r.typ) == Some(t) && !seen.contains(&to_i64(r.handle)) {
                seen.insert(to_i64(r.handle));
                list.push(r);
            }
        }
        table_recs.insert(t, list);
    }
    let mut cv = Conv {
        b: Build { ver, ctx, buf, records },
        ver,
        ms_rec,
        ps_rec,
        used: IntSet::default(),
        unknown: Obj::new(),
        build_errors: 0,
        mlatt_fold: IntSet::default(),
    };
    let empty: Vec<&Record> = Vec::new();
    let tr = |k: &str| table_recs.get(k).unwrap_or(&empty);
    let mut tables = Obj::new();
    // layers
    let mut layers = Vec::new();
    for r in tr("layers") {
        let lt_h = r.hn("ltype").unwrap_or(0.0);
        let mut o = cv.common_obj(r);
        o.set_opt("name", get_f(r, "name"));
        let flags = (if truthy(r, "frozen") { 1 } else { 0 })
            | (if truthy(r, "frozenNew") { 2 } else { 0 })
            | (if truthy(r, "locked") { 4 } else { 0 })
            | (if truthy(r, "_xdep") { 16 } else { 0 })
            | (if truthy(r, "_flag64") { 64 } else { 0 });
        o.set("flags", flags);
        o.set_opt("color", r.color.clone().map(V::Obj));
        o.set("linetype", hx(lt_h));
        o.set_opt("linetypeName", records.get(lt_h).and_then(|l| l.f.get("name").cloned()));
        o.set(
            "lineweight",
            match r.f.num("_lwIndex") {
                Some(i) => lineweight_from_index(to_i64(i)),
                None => -3,
            },
        );
        o.set_opt("plot", get_f(r, "plot"));
        o.set("off", !truthy(r, "on"));
        o.set_opt("frozen", get_f(r, "frozen"));
        o.set_opt("locked", get_f(r, "locked"));
        o.set_opt("frozenInNewViewports", get_f(r, "frozenNew"));
        for (k, hk) in [("plotStyle", "plotstyle"), ("material", "material")] {
            if let Some(h) = r.hn(hk)
                && h != 0.0
            {
                o.set(k, hx(h));
            }
        }
        layers.push(V::Obj(o));
    }
    tables.set("layers", layers);
    // linetypes
    let mut lts = Vec::new();
    for r in tr("linetypes") {
        let mut o = cv.common_obj(r);
        o.set_opt("name", get_f(r, "name"));
        o.set("flags", dep_flags(r));
        o.set_opt("description", get_f(r, "description"));
        o.set_opt("patternLength", get_f(r, "patternLength"));
        let shapes = r.hlist("shapes").unwrap_or_default();
        let els: Vec<V> =
            r.f.arr("elements")
                .ok_or_else(|| DwgError("Cannot read properties of undefined (reading 'map')".into()))?
                .iter()
                .enumerate()
                .map(|(i, e)| {
                    let e = e.as_obj().cloned().unwrap_or_default();
                    let mut x = Obj::new();
                    x.set_opt("length", e.get("length").cloned());
                    if e.truthy("flags") {
                        for k in ["flags", "shape"] {
                            x.set_opt(k, e.get(k).cloned());
                        }
                        x.set("style", hx(shapes.get(i).copied().unwrap_or(0.0)));
                        for k in ["offset", "scale", "rotation"] {
                            x.set_opt(k, e.get(k).cloned());
                        }
                        if let Some(t) = e.get("text") {
                            x.set("text", t.clone());
                        }
                    }
                    V::Obj(x)
                })
                .collect();
        o.set("elements", els);
        lts.push(V::Obj(o));
    }
    tables.set("linetypes", lts);
    // styles
    let mut styles = Vec::new();
    for r in tr("styles") {
        let mut o = cv.common_obj(r);
        o.set_opt("name", get_f(r, "name"));
        o.set("flags", (if truthy(r, "isShapeFile") { 1 } else { 0 }) | (if truthy(r, "vertical") { 4 } else { 0 }) | dep_flags(r));
        for k in ["height", "widthFactor", "oblique", "generation", "lastHeight", "font", "bigFont"] {
            o.set_opt(k, get_f(r, k));
        }
        o.set_opt("shapeFile", get_f(r, "isShapeFile"));
        styles.push(V::Obj(o));
    }
    tables.set("styles", styles);
    // dimstyles
    let mut dss = Vec::new();
    for r in tr("dimstyles") {
        let mut vars = r.f.obj("vars").cloned().unwrap_or_default();
        for k in ["DIMTXSTY", "DIMLDRBLK", "DIMBLK", "DIMBLK1", "DIMBLK2", "DIMLTYPE", "DIMLTEX1", "DIMLTEX2"] {
            if let Some(h) = r.hn(k) {
                vars.set(k, hx(h));
                if let Some(n) = cv.b.name_of(h) {
                    vars.set(format!("{k}_NAME"), n);
                }
            }
        }
        let mut o = cv.common_obj(r);
        o.set_opt("name", get_f(r, "name"));
        o.set("flags", dep_flags(r));
        o.set("vars", vars);
        dss.push(V::Obj(o));
    }
    tables.set("dimstyles", dss);
    for t in ["views", "ucs", "vports", "appids"] {
        let mut list = Vec::new();
        for r in tr(t) {
            let mut o = cv.common_obj(r);
            o.assign(plain(r));
            o.set("flags", dep_flags(r));
            list.push(V::Obj(o));
        }
        tables.set(t, list);
    }
    let mut brs = Vec::new();
    for r in tr("blockRecords") {
        let mut o = cv.common_obj(r);
        o.set_opt("name", get_f(r, "name"));
        o.set("flags", block_flags(r));
        o.set("layout", hx(r.hn("layout").unwrap_or(0.0)));
        for k in ["base", "xrefPath", "description"] {
            o.set_opt(k, get_f(r, k));
        }
        if let Some(t) = r.f.get("_trueName")
            && t.truthy()
        {
            o.set("trueName", t.clone());
        }
        if let Some(iu) = get_f(r, "insertUnits") {
            o.set("insertUnits", iu);
            o.set_opt("explodable", get_f(r, "explodable"));
            o.set_opt("scalable", get_f(r, "scalable"));
        }
        if let Some(p) = r.f.get("preview")
            && p.truthy()
        {
            o.set(
                "preview",
                match p {
                    V::Bytes(b) => V::Str(crate::util::b64(b)),
                    other => other.clone(),
                },
            );
        }
        brs.push(V::Obj(o));
    }
    tables.set("blockRecords", brs);
    model.set("tables", tables);
    ph("tables");

    // ---- entities
    let mut rec_space: IntMap<i64, &str> = IntMap::default();
    let mut layout_blocks: IntSet<i64> = IntSet::default();
    for r in records.iter() {
        if r.typ == "LAYOUT"
            && let Some(b) = r.hn("block")
            && b != 0.0
        {
            layout_blocks.insert(to_i64(b));
        }
    }
    for r in tr("blockRecords") {
        let name = r.f.get("name").map(js_string).unwrap_or_default();
        let lname = name.to_lowercase();
        if r.handle == ms_rec || lname == "*model_space" {
            rec_space.insert(to_i64(r.handle), "model");
        } else if r.handle == ps_rec
            || lname.starts_with("*paper_space")
            || (layout_blocks.contains(&to_i64(r.handle)) && name.starts_with('*'))
        {
            rec_space.insert(to_i64(r.handle), "paper");
        }
    }
    for r in records.iter() {
        if (r.typ != "ATTRIB" && r.typ != "ATTDEF") || r.hn("xdic").unwrap_or(0.0) == 0.0 {
            continue;
        }
        if let Some(data) = cv.mlatt_of(r) {
            for g in data {
                if g.0 == 340.0 {
                    cv.mlatt_fold.insert(to_i64(handle_num(&js_string(&g.1))));
                }
            }
        }
    }
    let mut counts = Obj::new();
    ph("pre-entities");
    let mut by_space = obj! { "model" => Obj::new(), "paper" => Obj::new(), "block" => Obj::new() };
    let count_type = |e: &Obj| -> String {
        if e.str("type") == Some("UNKNOWN") {
            e.get("dxfName").map(js_string).unwrap_or_default()
        } else {
            e.get("type").map(js_string).unwrap_or_default()
        }
    };
    let mut recs_in_order: Vec<&Record> = Vec::new();
    let br = tr("blockRecords");
    let ms = br.iter().copied().find(|r| r.handle == ms_rec);
    let ps = br.iter().copied().find(|r| r.handle == ps_rec);
    if let Some(m) = ms {
        recs_in_order.push(m);
    }
    if let Some(p) = ps
        && ms.is_none_or(|m| !std::ptr::eq(m, p))
    {
        recs_in_order.push(p);
    }
    for &r in br {
        if ms.is_some_and(|m| std::ptr::eq(m, r)) || ps.is_some_and(|p| std::ptr::eq(p, r)) {
            continue;
        }
        recs_in_order.push(r);
    }
    let mut blocks: Vec<Obj> = Vec::new();
    let mut pending: Vec<(Obj, Vec<f64>, Option<&str>, f64)> = Vec::new();
    for brr in recs_in_order {
        let blk = records.get(brr.hn("block").unwrap_or(0.0));
        let mut b = Obj::new();
        b.set_opt("name", get_f(brr, "name"));
        b.set("handle", hx(brr.hn("block").unwrap_or(0.0)));
        b.set("record", hx(brr.handle));
        b.set_opt("base", get_f(brr, "base"));
        b.set("flags", block_flags(brr));
        b.set_opt("xrefPath", get_f(brr, "xrefPath"));
        b.set(
            "description",
            match brr.f.get("description") {
                Some(v) if v.truthy() => v.clone(),
                _ => V::from(""),
            },
        );
        b.set("entities", Vec::<V>::new());
        b.set("endHandle", hx(brr.hn("endblk").unwrap_or(0.0)));
        if let Some(t) = brr.f.get("_trueName")
            && t.truthy()
        {
            b.set("trueName", t.clone());
        }
        if let Some(blk) = blk {
            cv.used.insert(to_i64(blk.handle));
            b.set("layer", cv.layer_name(blk.hn("layer").unwrap_or(0.0)));
            if let Some(xd) = cv.xdata_of(blk) {
                b.set("xdata", xd);
            }
        }
        if let Some(e) = brr.hn("endblk")
            && e != 0.0
        {
            cv.used.insert(to_i64(e));
        }
        let mut eh = brr.hlist("entities");
        if eh.is_none() && brr.hn("firstEntity").unwrap_or(0.0) != 0.0 {
            eh = Some(cv.chain(brr.hn("firstEntity").unwrap_or(0.0), brr.hn("lastEntity").unwrap_or(0.0)));
        }
        let space = rec_space.get(&to_i64(brr.handle)).copied();
        let mut todo: Vec<f64> = Vec::new();
        for h in eh.unwrap_or_default() {
            let Some(r) = records.get(h) else { continue };
            if !r.is_entity || SUB_TYPES.contains(&r.typ.as_str()) {
                continue;
            }
            if cv.mlatt_fold.contains(&to_i64(h)) {
                cv.used.insert(to_i64(h));
                continue;
            }
            todo.push(h);
        }
        pending.push((b, todo, space, brr.handle));
    }
    // every block's entities in one batch (parallel on native builds), then back in block order
    let flat: Vec<(f64, f64)> = pending.iter().flat_map(|(_, todo, _, owner)| todo.iter().map(move |&h| (h, *owner))).collect();
    ph("blocks-prep");
    let mut converted = convert_many(&cv, &flat).into_iter();
    ph("blocks-convert");
    for (mut b, todo, space, _) in pending {
        let mut ents = Vec::with_capacity(todo.len());
        for _ in 0..todo.len() {
            let Some((mut e, l)) = converted.next() else { break };
            cv.merge(l);
            if let Some(s) = space {
                e.set("space", s);
            }
            let s_key = space.unwrap_or("block");
            let space_counts = by_space.obj_mut(s_key);
            let t = count_type_ref(&e);
            inc(&mut counts, t);
            let mut space_counts = space_counts;
            if let Some(so) = space_counts.as_deref_mut() {
                inc(so, t);
            }
            if let Some(V::Arr(atts)) = e.get_mut("attribs") {
                for a in atts.iter_mut() {
                    if let V::Obj(a) = a {
                        if let Some(s) = space {
                            a.set("space", s);
                        }
                        let t = count_type_ref(a);
                        inc(&mut counts, t);
                        if let Some(so) = space_counts.as_deref_mut() {
                            inc(so, t);
                        }
                    }
                }
            }
            ents.push(V::Obj(e));
        }
        b.set("entities", ents);
        blocks.push(b);
    }
    ph("blocks");
    // orphans
    let mut orphans = 0usize;
    for r in records.iter() {
        if !r.is_entity || cv.used.contains(&to_i64(r.handle)) || SUB_TYPES.contains(&r.typ.as_str()) {
            continue;
        }
        let owner = cv.owner_of(r);
        let ohx = hx(owner);
        let Some(bi) = blocks.iter().position(|x| x.str("record") == Some(ohx.as_str())) else { continue };
        let mut l = Local::default();
        let mut e = cv.convert(&mut l, r.handle, owner);
        cv.merge(l);
        if let Some(s) = rec_space.get(&to_i64(owner)) {
            e.set("space", *s);
        }
        inc(&mut counts, &count_type(&e));
        if let Some(b) = blocks.get_mut(bi)
            && let Some(V::Arr(a)) = b.get_mut("entities")
        {
            a.push(V::Obj(e));
        }
        orphans = orphans.saturating_add(1);
    }
    if orphans > 0 {
        notes.push(msg("ccdwg.read.orphans", &[("n", &orphans.to_string())]));
    }
    ph("orphans");
    // layouts
    let mut layouts: Vec<Obj> = Vec::new();
    for r in records.iter() {
        if r.typ != "LAYOUT" {
            continue;
        }
        let p = r.f.obj("plot").cloned().unwrap_or_default();
        let block_rec = r.hn("block").unwrap_or(0.0);
        let vps = match r.hlist("viewports") {
            Some(v) => hlist(&v),
            None => {
                let bh = hx(block_rec);
                match blocks.iter().find(|x| x.str("record") == Some(bh.as_str())) {
                    Some(b) => V::Arr(
                        b.arr("entities")
                            .map(|a| {
                                a.iter()
                                    .filter_map(V::as_obj)
                                    .filter(|e| e.str("type") == Some("VIEWPORT"))
                                    .filter_map(|e| e.get("handle").cloned())
                                    .collect()
                            })
                            .unwrap_or_default(),
                    ),
                    None => V::Arr(Vec::new()),
                }
            }
        };
        let pg = |k: &str| p.get(k).cloned();
        let mut paper = Obj::new();
        paper.set_opt("name", pg("paperSize"));
        paper.set_opt("w", pg("paperWidth"));
        paper.set_opt("h", pg("paperHeight"));
        paper.set_opt("units", pg("paperUnits"));
        paper.set_opt("margins", pg("margins"));
        paper.set_opt("rotation", pg("rotation"));
        paper.set_opt("plotType", pg("plotType"));
        paper.set_opt("scaleType", pg("scaleType"));
        paper.set_opt("scaleFactor", pg("scaleFactor"));
        let du = p.num("drawingUnits");
        if p.truthy("drawingUnits") {
            paper.set("plotScale", p.num("realWorldUnits").unwrap_or(f64::NAN) / du.unwrap_or(f64::NAN));
        }
        paper.set_opt("realWorldUnits", pg("realWorldUnits"));
        paper.set_opt("drawingUnits", pg("drawingUnits"));
        for (k, pk) in [
            ("origin", "origin"),
            ("pageSetupName", "pageSetupName"),
            ("printer", "printer"),
            ("styleSheet", "styleSheet"),
            ("flags", "flags"),
            ("windowMin", "windowMin"),
            ("windowMax", "windowMax"),
        ] {
            paper.set_opt(k, pg(pk));
        }
        let mut o = cv.common_obj(r);
        o.set_opt("name", get_f(r, "name"));
        o.set_opt("tabOrder", get_f(r, "tabOrder"));
        o.set_opt("flags", get_f(r, "flags"));
        o.set("block", hx(block_rec));
        o.set("paper", paper);
        let mut lim = Obj::new();
        lim.set_opt("min", get_f(r, "limMin"));
        lim.set_opt("max", get_f(r, "limMax"));
        o.set("limits", lim);
        o.set_opt("insbase", get_f(r, "insBase"));
        let mut ext = Obj::new();
        ext.set_opt("min", get_f(r, "extMin"));
        ext.set_opt("max", get_f(r, "extMax"));
        o.set("extents", ext);
        let mut ucs = Obj::new();
        ucs.set_opt("origin", get_f(r, "ucsOrigin"));
        ucs.set_opt("xAxis", get_f(r, "ucsXAxis"));
        ucs.set_opt("yAxis", get_f(r, "ucsYAxis"));
        o.set("ucs", ucs);
        o.set_opt("elevation", get_f(r, "elevation"));
        o.set("lastActiveViewport", hx(r.hn("lastViewport").unwrap_or(0.0)));
        o.set("viewports", vps);
        layouts.push(o);
    }
    layouts.sort_by(|a, b| {
        let x = a.num("tabOrder").unwrap_or(f64::NAN);
        let y = b.num("tabOrder").unwrap_or(f64::NAN);
        (x - y).partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
    });
    ph("layouts");
    // non-graphical objects
    let is_table_thing = |t: &str| table_of(t).is_some() || t.ends_with("_CONTROL") || t == "VPENT_HEADER";
    let mut objects: Vec<Obj> = Vec::new();
    for r in records.iter() {
        if r.is_entity || is_table_thing(&r.typ) {
            continue;
        }
        let mut o = cv.common_obj(r);
        o.set("type", r.typ.clone());
        if r.unknown {
            o.set("undecoded", true);
            o.set("typeNum", r.type_num);
            if let Some(c) = &r.class_name
                && !c.is_empty()
            {
                o.set("className", c.clone());
            }
            inc(&mut cv.unknown, &r.typ);
        } else {
            match cv.object_fields(r) {
                Ok(f) => o.assign(f),
                Err(e) => {
                    o.set("damaged", format!("model: {}", e.0));
                    cv.build_errors = cv.build_errors.saturating_add(1);
                }
            }
        }
        if let Some(er) = &r.error {
            o.set("damaged", er.clone());
        }
        if let Some(raw) = crate::passthrough::capture_raw(r, ver, buf) {
            o.set("raw", raw);
        }
        objects.push(o);
    }
    ph("objects");
    objects.sort_by(|a, b| {
        let x = handle_num(a.str("handle").unwrap_or(""));
        let y = handle_num(b.str("handle").unwrap_or(""));
        x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal)
    });
    model.set("blocks", blocks.into_iter().map(V::Obj).collect::<Vec<V>>());
    model.set("layouts", layouts.into_iter().map(V::Obj).collect::<Vec<V>>());
    model.set("objects", objects.into_iter().map(V::Obj).collect::<Vec<V>>());
    *build_errors = build_errors.saturating_add(cv.build_errors);
    if let Some(rep) = model.obj_mut("report") {
        rep.set("counts", counts);
        rep.set("unknown", std::mem::take(&mut cv.unknown));
        rep.set("countsBySpace", by_space);
    }
    Ok(())
}

/// R13/R14 viewports keep their view in ACAD xdata "MVIEW".
fn mview_xdata(items: Option<&[(i32, V)]>) -> Obj {
    let Some(it) = items else { return Obj::new() };
    let Some(k) = it.iter().position(|g| g.0 == 1000 && g.1.as_str() == Some("MVIEW")) else { return Obj::new() };
    let v: Vec<V> = it.get(k.saturating_add(3)..).unwrap_or_default().iter().map(|g| g.1.clone()).collect();
    let g = |i: usize| v.get(i).cloned();
    let pt = |i: usize| match v.get(i) {
        Some(V::Obj(o)) => V::Obj(o.clone()),
        _ => crate::value::p3(0.0, 0.0, 0.0),
    };
    let pair = |a: usize, b: usize| {
        let mut o = Obj::new();
        o.set_opt("x", g(a));
        o.set_opt("y", g(b));
        V::Obj(o)
    };
    let mut o = Obj::new();
    o.set("viewTarget", pt(0));
    o.set("viewDirection", pt(1));
    o.set_opt("twist", g(2));
    o.set_opt("viewHeight", g(3));
    o.set("viewCenter", pair(4, 5));
    o.set_opt("lensLength", g(6));
    o.set_opt("frontClip", g(7));
    o.set_opt("backClip", g(8));
    o.set_opt("viewMode", g(9));
    o.set_opt("circleZoom", g(10));
    o.set_opt("snapAngle", g(17));
    o.set("snapBase", pair(18, 19));
    o.set("snapSpacing", pair(20, 21));
    o.set("gridSpacing", pair(22, 23));
    o
}

fn pz(o: &V) -> [f64; 3] {
    let ob = o.as_obj();
    let g = |k: &str| ob.and_then(|x| x.num(k)).unwrap_or(f64::NAN);
    let z = ob.and_then(|x| x.num("z")).unwrap_or(0.0);
    [g("x"), g("y"), if z.is_nan() { 0.0 } else { z }]
}

/// Control points of a spline stored by fit points only (read.js fitSplineControl; see the spec-gap note there).
#[allow(clippy::many_single_char_names, clippy::too_many_lines)]
pub fn fit_spline_control(fit: &[V], degree: f64, t0: Option<&Obj>, t1: Option<&Obj>, knot_param: f64) -> Option<(Vec<f64>, Vec<V>)> {
    let n = fit.len();
    let p = if degree == 0.0 || degree.is_nan() { 3.0 } else { degree };
    if p != 3.0 || !(2..=2000).contains(&n) {
        return None;
    }
    let pts: Vec<[f64; 3]> = fit.iter().map(pz).collect();
    let at = |i: usize| pts.get(i).copied().unwrap_or([0.0; 3]);
    let mut u = vec![0.0f64];
    for i in 1..n {
        let a = at(i.saturating_sub(1));
        let b = at(i);
        let mut d = js_hypot3(b[0] - a[0], b[1] - a[1], b[2] - a[2]);
        if knot_param == 1.0 {
            d = d.sqrt();
        } else if knot_param == 2.0 {
            d = 1.0;
        }
        let prev = u.last().copied().unwrap_or(0.0);
        u.push(prev + d);
    }
    let l = u.last().copied().unwrap_or(0.0);
    if l.is_nan() || l <= 0.0 {
        return None;
    }
    let mut knots = vec![0.0, 0.0, 0.0, 0.0];
    knots.extend(u.get(1..n.saturating_sub(1)).unwrap_or_default());
    knots.extend([l, l, l, l]);
    // 2 <= n <= 2000, so m - 3 >= 1 and every index below is small and non-negative.
    let m = n.saturating_add(2);
    let kn = |i: usize| knots.get(i).copied().unwrap_or(0.0);
    let basis = |t: f64| -> (usize, [f64; 4]) {
        let mut span = 3usize;
        while span < m.saturating_sub(1) && t >= kn(span.saturating_add(1)) {
            span = span.saturating_add(1);
        }
        let mut nn = [1.0, 0.0, 0.0, 0.0];
        let mut left = [0.0f64; 4];
        let mut right = [0.0f64; 4];
        for j in 1..=3usize {
            if let Some(x) = left.get_mut(j) {
                *x = t - kn(span.saturating_add(1).saturating_sub(j)); // span >= 3 >= j
            }
            if let Some(x) = right.get_mut(j) {
                *x = kn(span.saturating_add(j)) - t;
            }
            let mut saved = 0.0;
            for r in 0..j {
                let rr = right.get(r.saturating_add(1)).copied().unwrap_or(0.0);
                let ll = left.get(j.saturating_sub(r)).copied().unwrap_or(0.0);
                let tmp = nn.get(r).copied().unwrap_or(0.0) / (rr + ll);
                if let Some(x) = nn.get_mut(r) {
                    *x = saved + rr * tmp;
                }
                saved = ll * tmp;
            }
            if let Some(x) = nn.get_mut(j) {
                *x = saved;
            }
        }
        (span, nn)
    };
    let mut a = vec![vec![0.0f64; m]; m];
    let mut bm = vec![[0.0f64; 3]; m];
    let set = |a: &mut Vec<Vec<f64>>, r: usize, c: usize, v: f64| {
        if let Some(x) = a.get_mut(r).and_then(|row| row.get_mut(c)) {
            *x = v;
        }
    };
    let has = |t: Option<&Obj>| t.is_some_and(|t| t.truthy("x") || t.truthy("y") || t.truthy("z"));
    let tv = |t: Option<&Obj>| {
        let g = |k: &str| t.and_then(|x| x.num(k)).unwrap_or(0.0);
        [g("x"), g("y"), g("z")]
    };
    set(&mut a, 0, 0, 1.0);
    if let Some(x) = bm.get_mut(0) {
        *x = at(0);
    }
    let u4 = kn(4);
    let u5 = kn(5);
    if has(t0) {
        let t = tv(t0);
        set(&mut a, 1, 0, -1.0);
        set(&mut a, 1, 1, 1.0);
        if let Some(x) = bm.get_mut(1) {
            *x = [t[0] * u4 / 3.0, t[1] * u4 / 3.0, t[2] * u4 / 3.0];
        }
    } else {
        let d = if u5 - kn(1) != 0.0 { u5 - kn(1) } else { u5 };
        set(&mut a, 1, 0, 1.0 / u4);
        set(&mut a, 1, 1, -1.0 / u4 - 1.0 / d);
        set(&mut a, 1, 2, 1.0 / d);
    }
    for k in 1..n.saturating_sub(1) {
        let (span, nn) = basis(u.get(k).copied().unwrap_or(0.0));
        for (j, v) in nn.iter().enumerate() {
            set(&mut a, k.saturating_add(1), span.saturating_add(j).saturating_sub(3), *v); // span >= 3
        }
        if let Some(x) = bm.get_mut(k.saturating_add(1)) {
            *x = at(k);
        }
    }
    let (m1, m2, m3) = (m.saturating_sub(1), m.saturating_sub(2), m.saturating_sub(3));
    let ua = kn(m1);
    let ub = kn(m2);
    let e1 = l - ua;
    let e2 = l - ub;
    if has(t1) {
        let t = tv(t1);
        set(&mut a, m2, m2, -1.0);
        set(&mut a, m2, m1, 1.0);
        if let Some(x) = bm.get_mut(m2) {
            *x = [t[0] * e1 / 3.0, t[1] * e1 / 3.0, t[2] * e1 / 3.0];
        }
    } else {
        set(&mut a, m2, m1, 1.0 / e1);
        set(&mut a, m2, m2, -1.0 / e1 - 1.0 / e2);
        set(&mut a, m2, m3, 1.0 / e2);
    }
    set(&mut a, m1, m1, 1.0);
    if let Some(x) = bm.get_mut(m1) {
        *x = at(n.saturating_sub(1));
    }
    let g = |a: &Vec<Vec<f64>>, r: usize, c: usize| a.get(r).and_then(|row| row.get(c)).copied().unwrap_or(0.0);
    for c in 0..m {
        let mut piv = c;
        for r in c.saturating_add(1)..m.min(c.saturating_add(5)) {
            if g(&a, r, c).abs() > g(&a, piv, c).abs() {
                piv = r;
            }
        }
        if g(&a, piv, c) == 0.0 {
            return None;
        }
        if piv != c {
            a.swap(piv, c);
            bm.swap(piv, c);
        }
        for r in c.saturating_add(1)..m.min(c.saturating_add(5)) {
            let f = g(&a, r, c) / g(&a, c, c);
            if f == 0.0 {
                continue;
            }
            for k in c..m.min(c.saturating_add(6)) {
                let v = g(&a, r, k) - f * g(&a, c, k);
                set(&mut a, r, k, v);
            }
            let bc = bm.get(c).copied().unwrap_or([0.0; 3]);
            if let Some(br) = bm.get_mut(r) {
                for q in 0..3 {
                    if let (Some(x), Some(y)) = (br.get_mut(q), bc.get(q)) {
                        *x -= f * y;
                    }
                }
            }
        }
    }
    let mut pp = vec![[0.0f64; 3]; m];
    for r in (0..m).rev() {
        let mut v = bm.get(r).copied().unwrap_or([0.0; 3]);
        for k in r.saturating_add(1)..m.min(r.saturating_add(6)) {
            let akr = g(&a, r, k);
            if akr != 0.0 {
                let pk = pp.get(k).copied().unwrap_or([0.0; 3]);
                for q in 0..3 {
                    if let (Some(x), Some(y)) = (v.get_mut(q), pk.get(q)) {
                        *x -= akr * y;
                    }
                }
            }
        }
        let d = g(&a, r, r);
        if let Some(x) = pp.get_mut(r) {
            *x = [v[0] / d, v[1] / d, v[2] / d];
        }
    }
    Some((knots, pp.into_iter().map(|q| crate::value::p3(q[0], q[1], q[2])).collect()))
}

/// V8's Math.hypot for three finite values (scaled by the largest magnitude, compensated sum).
pub fn js_hypot3(a: f64, b: f64, c: f64) -> f64 {
    let (a, b, c) = (a.abs(), b.abs(), c.abs());
    if a.is_infinite() || b.is_infinite() || c.is_infinite() {
        return f64::INFINITY;
    }
    if a.is_nan() || b.is_nan() || c.is_nan() {
        return f64::NAN;
    }
    let max = a.max(b).max(c);
    if max == 0.0 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    let mut comp = 0.0f64;
    for v in [a, b, c] {
        let r = v / max;
        let s = r * r - comp;
        let p = sum + s;
        comp = (p - sum) - s;
        sum = p;
    }
    sum.sqrt() * max
}
