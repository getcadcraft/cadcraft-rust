//! Raw passthrough of objects and entities cc-dwg does not decode, writing side (port of lib/dwg/passthrough.js:
//! encodeRaw, rawEntityCommon, appendBits, frameObject, classData, convertPlan, encodeConverted).
//!
//! The common part is written again for the target file and the data bits are copied verbatim; every handle
//! reference is mapped to the written file's handle (0 when the target was not written). Relative references become
//! absolute soft pointers (code 4).
//! Spec gap: R2000–R2018 proxies of undecoded classes (20.4.90): class id BL, [R2004: "cn:" + C++ class name as T],
//! data format BL (R2018: version BL + maintenance BL), original format B, then the data in its R2007 form — the
//! layout found by comparing the ODA File Converter's proxies with the native objects (see passthrough.js).

use crate::bits::{BitReader, Codepage, Res};
use crate::util::{to_usize, unb64, usize_to_f64};
use crate::value::{Obj, V};
use crate::w_bits::{BitWriter, append_bits, version_num};
use crate::w_js::{arr, field, int32, js_string, num, str_or, to_number};
use crate::w_objects::{ClassProps, Cx, Rec, SOFT_PTR, begin, entity_common_rest, frame_object, object_common};

/// Versions whose data of a class is bit for bit the same (passthrough.js RAW_GROUPS; Spec gap, found by comparing
/// every object of these classes across the corpus).
fn raw_groups(ty: &str) -> &'static [&'static [u32]] {
    const TWO: &[&[u32]] = &[&[15, 18], &[21, 24, 27, 32]];
    match ty {
        "MATERIAL" | "DIMASSOC" | "ACDB_ALDIMOBJECTCONTEXTDATA_CLASS" | "ACDB_MTEXTOBJECTCONTEXTDATA_CLASS" | "RAPIDRTRENDERSETTINGS" => {
            TWO
        }
        "SUN" => &[&[21, 24, 27, 32]],
        "VISUALSTYLE" => &[&[27, 32]],
        "ACDB_MLEADEROBJECTCONTEXTDATA_CLASS" => &[&[24, 27, 32]],
        "ACDBDETAILVIEWSTYLE" | "ACDBSECTIONVIEWSTYLE" => &[&[21, 24, 27]],
        _ => &[],
    }
}
/// rawUsable(raw, version, type)
pub fn raw_usable(raw: Option<&V>, version: &str, ty: Option<&str>) -> bool {
    let Some(raw) = raw.filter(|r| r.truthy()) else { return false };
    let v = f64::from(version_num(version));
    let rv = to_number(field(Some(raw), "ver"));
    if matches!(field(Some(raw), "ver"), Some(V::Num(_))) && rv == v {
        return true;
    }
    let Some(t) = ty else { return false };
    raw_groups(t).iter().any(|g| g.iter().any(|&x| f64::from(x) == rv) && g.iter().any(|&x| f64::from(x) == v))
}

/// The class data of a raw object in the version-free R2007 form.
#[derive(Clone, Debug)]
pub struct ClassData {
    pub bits: Vec<u8>,
    pub nd: usize,
    pub strings: Vec<Vec<u16>>,
    pub fmt_ver: f64,
    pub fmt_maint: f64,
    pub class_id: Option<f64>,
}
#[derive(Clone, Debug)]
pub struct Plan {
    pub native: bool,
    pub data: ClassData,
    pub cls: ClassProps,
}
/// A raw record (rec.encode = encodeRaw).
#[derive(Clone, Debug)]
pub struct RawRec {
    pub type_num: Option<f64>,
    pub cls: Option<ClassProps>,
    pub raw: Obj,
    pub is_entity: bool,
    pub proxy_class: bool,
    pub conv: Option<Plan>,
}

fn dhl(version: &str) -> f64 {
    match version {
        "AC1015" => 23.0,
        "AC1018" => 25.0,
        "AC1021" => 27.0,
        "AC1024" => 29.0,
        "AC1027" => 31.0,
        "AC1032" => 33.0,
        _ => f64::NAN,
    }
}
fn dhl_ver_ok(v: f64) -> bool {
    [23.0, 25.0, 27.0, 29.0, 31.0, 33.0].contains(&v)
}

/// bits-read.js TU on raw units (trailing NULs dropped), version 32: no \U+ decoding.
fn tu_units(r: &mut BitReader) -> Res<Vec<u16>> {
    let n = crate::util::usize_from_u32(crate::util::u32_from_i32(r.bs()?) & 0xFFFF);
    if n == 0 {
        return Ok(Vec::new());
    }
    if r.pos.saturating_add(n.saturating_mul(16)) > r.end {
        return crate::bits::err("read past the end");
    }
    let mut u = Vec::with_capacity(n);
    for _ in 0..n {
        let lo = r.rc()?;
        let hi = r.rc()?;
        u.push(crate::util::u16_of_i32(crate::util::i32_from_u32((hi << 8) | lo)));
    }
    while u.last() == Some(&0) {
        u.pop();
    }
    Ok(u)
}
/// The R2007-form tail of a blob: (data bits, strings).
fn strings_of(buf: &[u8], n: usize) -> Option<(usize, Vec<Vec<u16>>)> {
    if n == 0 {
        return Some((0, Vec::new()));
    }
    let mut t = BitReader::range(buf, 15, Codepage::latin1(), n.saturating_sub(1), n);
    if t.b().ok()? == 0 {
        return Some((n.saturating_sub(1), Vec::new()));
    }
    let mut p = n.checked_sub(17)?;
    t.pos = p;
    t.end = p.saturating_add(16);
    let mut size = crate::util::usize_from_u32(t.rsu().ok()?);
    if size & 0x8000 != 0 {
        p = p.checked_sub(16)?;
        t.pos = p;
        t.end = p.saturating_add(16);
        let hi = crate::util::usize_from_u32(t.rsu().ok()?);
        size = (size & 0x7FFF) | (hi << 15);
    }
    let start = p.checked_sub(size)?;
    let mut s = BitReader::range(buf, 32, Codepage::latin1(), start, p);
    let mut strings = Vec::new();
    while s.end.saturating_sub(s.pos) >= 2 && strings.len() < 1_000_000 {
        strings.push(tu_units(&mut s).ok()?);
    }
    Some((start, strings))
}

/// passthrough.js classData
pub fn class_data(raw: &Obj, is_proxy: bool, src_version: &str, src_maint: f64) -> Option<ClassData> {
    let buf = unb64(raw.str("data").unwrap_or(""));
    let n = to_usize(num(raw.get("n"), 0.0)).min(buf.len().saturating_mul(8));
    let v = num(raw.get("ver"), 0.0);
    if !is_proxy {
        if v < 21.0 {
            return None;
        }
        let (nd, strings) = strings_of(&buf, n)?;
        return Some(ClassData { bits: buf, nd, strings, fmt_ver: dhl(src_version), fmt_maint: src_maint, class_id: None });
    }
    let vu = crate::util::to_u32(v);
    let mut r = BitReader::range(&buf, vu, Codepage::latin1(), 0, n);
    let class_id = f64::from(r.bl().ok()?);
    let (fv, fm) = if vu == 18 {
        r.t().ok()?;
        let f = crate::util::u32_from_i32(r.bl().ok()?);
        (f64::from(f & 0xFFFF), f64::from(f >> 16))
    } else if vu >= 32 {
        let a = f64::from(r.bl().ok()?);
        let b = f64::from(r.bl().ok()?);
        (a, b)
    } else {
        let f = crate::util::u32_from_i32(r.bl().ok()?);
        (f64::from(f & 0xFFFF), f64::from(f >> 16))
    };
    r.b().ok()?;
    let body = crate::util::slice_bits(&buf, r.pos, n);
    let nb = n.saturating_sub(r.pos);
    let (nd, mut strings) = strings_of(&body, nb)?;
    if vu >= 21 && strings.first().is_some_and(|s| s.starts_with(&[0x63, 0x6E, 0x3A])) {
        strings.remove(0);
    }
    Some(ClassData { bits: body, nd, strings, fmt_ver: fv, fmt_maint: fm, class_id: Some(class_id) })
}

/// passthrough.js convertPlan
pub fn convert_plan(
    raw: Option<&Obj>,
    is_proxy: bool,
    cls_for: &dyn Fn(Option<f64>) -> Option<ClassProps>,
    src_version: &str,
    src_maint: f64,
    version: &str,
) -> Option<Plan> {
    let raw = raw?;
    let data = class_data(raw, is_proxy, src_version, src_maint)?;
    if data.fmt_ver == 0.0 || data.fmt_ver.is_nan() || !dhl_ver_ok(data.fmt_ver) {
        return None;
    }
    let cls = cls_for(data.class_id)?;
    let native = data.fmt_ver == dhl(version) && version_num(version) >= 21;
    Some(Plan { native, data, cls })
}

/// Map one source handle reference.
fn map_ref(cx: &Cx, code: f64, hex: Option<&V>) -> (u32, f64) {
    let c = crate::util::u32_from_i32(crate::util::to_i32(code));
    let empty = match hex {
        None | Some(V::Null) => true,
        Some(V::Str(s)) => s.is_empty() || s == "0",
        Some(x) => !x.truthy(),
    };
    if empty {
        return (c, 0.0);
    }
    let v = cx.env.map(hex);
    (if code >= 6.0 { SOFT_PTR } else { c }, v)
}
fn write_handles(w: &mut BitWriter, raw: &Obj, cx: &Cx) {
    for it in arr(raw.get("h")) {
        let pair = it.as_arr().map(Vec::as_slice).unwrap_or(&[]);
        let (c, v) = map_ref(cx, to_number(pair.first()), pair.get(1));
        if let Some(h) = w.hnd.as_mut() {
            h.h_data(c, v);
        }
    }
}

/// `{ dxf, cpp, app, entity: !!, proxy: | 0 }` of a class (registered under 'RAW:' + dxf).
fn reg(cx: &mut Cx, cls: &ClassProps) -> String {
    let key = cls.raw_key();
    cx.classes.register(&key, cls.clone());
    key
}
/// rawType: the type number of a raw record in the written file.
fn raw_type(rr: &RawRec, cx: &mut Cx) -> f64 {
    let tn = rr.type_num.unwrap_or(f64::NAN);
    if tn < 500.0 {
        return tn;
    }
    let cls = rr.cls.clone().unwrap_or_default();
    let key = reg(cx, &cls);
    cx.classes.class_num(&key)
}

/// passthrough.js rawEntityCommon (the source's proxy graphics).
pub fn raw_entity_common(w: &mut BitWriter, r: &Rec, gfx: Option<&V>) {
    let g = match gfx {
        Some(V::Str(s)) if !s.is_empty() => unb64(s),
        _ => Vec::new(),
    };
    if !g.is_empty() {
        w.b(true);
        if w.r2010 {
            w.bll(usize_to_f64(g.len()));
        } else {
            w.rl(usize_to_f64(g.len()));
        }
        for b in g {
            w.rc8(b);
        }
    } else {
        w.b(false);
    }
    entity_common_rest(w, r, true);
}

pub fn encode_raw(rr: &RawRec, r: &Rec, cx: &mut Cx) -> Option<Vec<u8>> {
    if let Some(plan) = &rr.conv {
        return Some(encode_converted(rr, plan, r, cx));
    }
    let raw = &rr.raw;
    let mut w = BitWriter::obj_writer(cx.version, false);
    let ty = raw_type(rr, cx);
    begin(&mut w, ty, r);
    if rr.is_entity {
        raw_entity_common(&mut w, r, raw.get("gfx"));
    } else {
        object_common(&mut w, r);
    }
    let mut bits = unb64(&crate::w_js::str_or(raw.get("data"), ""));
    let mut n = crate::w_bits::js_bit_count(raw.get("n"), bits.len().saturating_mul(8));
    // 20.4.90: a proxy's data starts with its class id (BL); classes are renumbered in the written file
    let tn = rr.type_num.unwrap_or(f64::NAN);
    if (tn == 498.0 || tn == 499.0) && rr.proxy_class && cx.ver >= 15 {
        let mut rd = BitReader::range(&bits, cx.ver, Codepage::latin1(), 0, n);
        if let Ok(id) = rd.bl()
            && let Some(c) = cx.env.cls_of(f64::from(id))
        {
            {
                let key = reg(cx, &c);
                let mut t = BitWriter::obj_writer(cx.version, false);
                let cn = cx.classes.class_num(&key);
                t.bl(cn);
                let rest = crate::util::slice_bits(&bits, rd.pos, n);
                append_bits(&mut t, &rest, n.saturating_sub(rd.pos));
                bits = t.buf;
                n = t.bit;
            }
        }
    }
    append_bits(&mut w, &bits, n);
    let endbit = w.bit;
    write_handles(&mut w, raw, cx);
    Some(frame_object(&mut w, endbit))
}

/// The bits of a string stream in its R2007 form: TU strings, size, 1 (or a 0 bit for none).
fn write_strings(w: &mut BitWriter, strings: &[Vec<u16>]) {
    if strings.is_empty() {
        w.b(false);
        return;
    }
    let mut t = BitWriter::obj_writer("AC1021", false);
    for s in strings {
        t.tu_units(s);
    }
    append_bits(w, &t.buf, t.bit);
    let n = t.bit;
    if n < 0x8000 {
        w.rs_i(crate::util::i32_of_usize(n));
    } else {
        w.rs_i(crate::util::i32_of_usize(n >> 15));
        w.rs_i(crate::util::i32_of_usize((n & 0x7FFF) | 0x8000));
    }
    w.b(true);
}

fn encode_converted(rr: &RawRec, plan: &Plan, r: &Rec, cx: &mut Cx) -> Vec<u8> {
    let v = cx.ver;
    let mut w = BitWriter::obj_writer(cx.version, false);
    let cls = rr.cls.clone().unwrap_or_else(|| plan.cls.clone());
    let key = reg(cx, &cls);
    let cnum = cx.classes.class_num(&key);
    let ty = if plan.native {
        cnum
    } else if rr.is_entity {
        498.0
    } else {
        499.0
    };
    begin(&mut w, ty, r);
    if rr.is_entity {
        raw_entity_common(&mut w, r, rr.raw.get("gfx"));
    } else {
        object_common(&mut w, r);
    }
    let data = &plan.data;
    let cpp = js_string(cls.cpp.as_ref());
    if plan.native {
        append_bits(&mut w, &data.bits, data.nd);
        for s in &data.strings {
            w.tv_units(s);
        }
        let endbit = w.end_data();
        write_handles(&mut w, &rr.raw, cx);
        return frame_object(&mut w, endbit);
    }
    let fmt = (crate::util::u32_from_i32(crate::util::to_i32(data.fmt_maint)) & 0xFFFF) << 16 | crate::util::to_u32(data.fmt_ver);
    w.bl(cnum);
    if v == 18 {
        let cn = format!("cn:{cpp}");
        let u: Vec<u16> = cn.encode_utf16().collect();
        w.bs(usize_to_f64(u.len()));
        for c in u {
            w.rc8(crate::util::u8_of(u32::from(c)));
        }
    }
    if v >= 32 {
        w.bl(data.fmt_ver);
        w.bl(data.fmt_maint);
    } else {
        w.bl(f64::from(crate::util::i32_from_u32(fmt)));
    }
    w.b(false);
    append_bits(&mut w, &data.bits, data.nd);
    let endbit = if v < 21 {
        write_strings(&mut w, &data.strings);
        w.bit
    } else {
        w.tv(&format!("cn:{cpp}"));
        for s in &data.strings {
            w.tv_units(s);
        }
        w.end_data()
    };
    write_handles(&mut w, &rr.raw, cx);
    frame_object(&mut w, endbit)
}

/// `/^(MATERIAL|VISUALSTYLE|…)$/` (nongraphic-write.js NEVER_PROXY)
pub fn never_proxy(s: &str) -> bool {
    let w = |x: &str| !x.is_empty() && x.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    let w0 = |x: &str| x.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    if matches!(
        s,
        "MATERIAL"
            | "VISUALSTYLE"
            | "ACDBDETAILVIEWSTYLE"
            | "ACDBSECTIONVIEWSTYLE"
            | "SUN"
            | "DIMASSOC"
            | "RAPIDRTRENDERSETTINGS"
            | "TABLESTYLE"
            | "CELLSTYLEMAP"
            | "TABLECONTENT"
            | "TABLEGEOMETRY"
            | "DATATABLE"
            | "FIELD"
            | "FIELDLIST"
            | "SCALE"
            | "MLEADERSTYLE"
            | "LAYOUT"
            | "PLOTSETTINGS"
            | "XRECORD"
            | "GROUP"
            | "ACDBPLACEHOLDER"
            | "SORTENTSTABLE"
            | "LAYER_INDEX"
            | "IDBUFFER"
            | "DBCOLOR"
            | "GEODATA"
            | "LIGHTLIST"
            | "MULTILEADER"
            | "ACAD_TABLE"
            | "LIGHT"
            | "REGION"
            | "3DSOLID"
            | "BODY"
            | "MLINESTYLE"
    ) {
        return true;
    }
    if let Some(r) = s.strip_prefix("RENDER") {
        return w0(r);
    }
    if let Some(r) = s.strip_prefix("ACDB_")
        && let Some(m) = r.strip_suffix("OBJECTCONTEXTDATA_CLASS")
    {
        return w0(m);
    }
    if let Some(m) = s.strip_suffix("VARIABLES") {
        return w0(m);
    }
    if let Some(r) = s.strip_prefix("IMAGEDEF") {
        return w0(r);
    }
    if let Some(r) = s.strip_prefix("DICTIONARY") {
        return w0(r);
    }
    if let Some(r) = s.strip_prefix("SPATIAL_") {
        return w(r);
    }
    false
}

/// `{ dxf: c.dxfName, … }` of a model class value
pub fn class_props_of(c: Option<&V>) -> ClassProps {
    ClassProps {
        dxf: field(c, "dxfName").cloned(),
        cpp: field(c, "cppName").cloned(),
        app: field(c, "appName").cloned(),
        entity: crate::w_js::truthy(field(c, "isEntity")),
        proxy: int32(field(c, "proxyFlags")),
    }
}
/// String of a class's dxf name (`c.dxf`)
pub fn dxf_of(c: &ClassProps) -> String {
    str_or(c.dxf.as_ref(), "undefined")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn never_proxy_list() {
        assert!(never_proxy("MATERIAL"));
        assert!(never_proxy("RENDERSETTINGS"));
        assert!(never_proxy("ACDB_MTEXTOBJECTCONTEXTDATA_CLASS"));
        assert!(never_proxy("WIPEOUTVARIABLES"));
        assert!(never_proxy("SPATIAL_FILTER"));
        assert!(!never_proxy("SPATIAL_"));
        assert!(!never_proxy("ACAD_EVALUATION_GRAPH"));
    }
}
