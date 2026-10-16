//! DWG object, header-variable and class encoders for every version R2000–R2018 (port of lib/dwg/objects-write.js,
//! Apache-2.0; ODA spec ch. 2, 9, 10, 20, 28). The "Spec gap:" notes of the JS sit next to the code they explain.
//!
//! Input: records prepared by the writer (w_write.rs) — plain objects (`Rec.o`, the JS record's fields) with numeric
//! handles already resolved, plus the custom encoder of records other modules build (`Rec.enc`).
//! Output: the bytes of one object as stored in the AcDbObjects stream (MS size, [R2010+ handle-stream size],
//! bit data, CRC-16).

use std::collections::HashMap;

use crate::crc::crc8 as crc16;
use crate::header_vars::{HEADER_VARS, T as HT, header_def};
use crate::obj;
use crate::util::{to_i32, usize_to_f64};
use crate::value::{Obj, V};
use crate::w_bits::{BitWriter, ByteWriter, clean_units, encode1252, umc_bytes, version_num};
use crate::w_js::{arr, field, hex_key, int, int32, js_string, nn, num, parse_int16, str_or, to_number, truthy, units};

pub const SOFT_OWN: u32 = 2;
pub const HARD_OWN: u32 = 3;
pub const SOFT_PTR: u32 = 4;
pub const HARD_PTR: u32 = 5;

/// Fixed object types.
pub fn fixed_type(t: &str) -> Option<f64> {
    Some(match t {
        "TEXT" => 1.0,
        "ATTRIB" => 2.0,
        "ATTDEF" => 3.0,
        "BLOCK" => 4.0,
        "ENDBLK" => 5.0,
        "SEQEND" => 6.0,
        "INSERT" => 7.0,
        "MINSERT" => 8.0,
        "VERTEX_2D" => 10.0,
        "VERTEX_3D" => 11.0,
        "VERTEX_MESH" => 12.0,
        "VERTEX_PFACE" => 13.0,
        "VERTEX_PFACE_FACE" => 14.0,
        "POLYLINE_2D" => 15.0,
        "POLYLINE_3D" => 16.0,
        "ARC" => 17.0,
        "CIRCLE" => 18.0,
        "LINE" => 19.0,
        "DIMENSION_ORDINATE" => 20.0,
        "DIMENSION_LINEAR" => 21.0,
        "DIMENSION_ALIGNED" => 22.0,
        "DIMENSION_ANG3PT" => 23.0,
        "DIMENSION_ANG2LN" => 24.0,
        "DIMENSION_RADIUS" => 25.0,
        "DIMENSION_DIAMETER" => 26.0,
        "POINT" => 27.0,
        "3DFACE" => 28.0,
        "POLYLINE_PFACE" => 29.0,
        "POLYLINE_MESH" => 30.0,
        "SOLID" => 31.0,
        "TRACE" => 32.0,
        "SHAPE" => 33.0,
        "VIEWPORT" => 34.0,
        "ELLIPSE" => 35.0,
        "SPLINE" => 36.0,
        "REGION" => 37.0,
        "3DSOLID" => 38.0,
        "BODY" => 39.0,
        "RAY" => 40.0,
        "XLINE" => 41.0,
        "DICTIONARY" => 42.0,
        "MTEXT" => 44.0,
        "LEADER" => 45.0,
        "TOLERANCE" => 46.0,
        "MLINE" => 47.0,
        "BLOCK_CONTROL" => 48.0,
        "BLOCK_HEADER" => 49.0,
        "LAYER_CONTROL" => 50.0,
        "LAYER" => 51.0,
        "STYLE_CONTROL" => 52.0,
        "STYLE" => 53.0,
        "LTYPE_CONTROL" => 56.0,
        "LTYPE" => 57.0,
        "VIEW_CONTROL" => 60.0,
        "VIEW" => 61.0,
        "UCS_CONTROL" => 62.0,
        "UCS" => 63.0,
        "VPORT_CONTROL" => 64.0,
        "VPORT" => 65.0,
        "APPID_CONTROL" => 66.0,
        "APPID" => 67.0,
        "DIMSTYLE_CONTROL" => 68.0,
        "DIMSTYLE" => 69.0,
        "VPENT_CONTROL" => 70.0,
        "VPENT" => 71.0,
        "GROUP" => 72.0,
        "MLINESTYLE" => 73.0,
        "LWPOLYLINE" => 77.0,
        "HATCH" => 78.0,
        "XRECORD" => 79.0,
        "PLACEHOLDER" => 80.0,
        "LAYOUT" => 82.0,
        _ => return None,
    })
}

// ---------------------------------------------------------------- classes (CLASSED)
/// Class properties (DXF CLASSES section names): `dxf`, `cpp`, `app` are JS values (a model class may lack them).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClassProps {
    pub dxf: Option<V>,
    pub cpp: Option<V>,
    pub app: Option<V>,
    pub entity: bool,
    pub proxy: i32,
}
pub fn cp(dxf: &str, cpp: &str, app: &str, entity: bool, proxy: i32) -> ClassProps {
    ClassProps { dxf: Some(V::from(dxf)), cpp: Some(V::from(cpp)), app: Some(V::from(app)), entity, proxy }
}
impl ClassProps {
    /// `'RAW:' + c.dxf`
    pub fn raw_key(&self) -> String {
        format!("RAW:{}", js_string(self.dxf.as_ref()))
    }
}
/// The CLASSED table every module registers into (objects-write.js, nongraphic-write.js, mleader-write.js,
/// table-write.js, solid-write.js, surface-write.js), as it stands once all modules are loaded.
pub fn classed_init() -> HashMap<String, ClassProps> {
    let mut m = HashMap::new();
    let mut add = |k: &str, c: ClassProps| {
        m.entry(k.to_string()).or_insert(c);
    };
    add("LAYOUT", cp("LAYOUT", "AcDbLayout", "ObjectDBX Classes", false, 0));
    add("DICTIONARYWDFLT", cp("ACDBDICTIONARYWDFLT", "AcDbDictionaryWithDefault", "ObjectDBX Classes", false, 0));
    add("PLACEHOLDER", cp("ACDBPLACEHOLDER", "AcDbPlaceHolder", "ObjectDBX Classes", false, 0));
    add("XRECORD", cp("XRECORD", "AcDbXrecord", "ObjectDBX Classes", false, 0));
    add("ARC_DIMENSION", cp("ARC_DIMENSION", "AcDbArcDimension", "ObjectDBX Classes", true, 0x401));
    add("LARGE_RADIAL_DIMENSION", cp("LARGE_RADIAL_DIMENSION", "AcDbRadialDimensionLarge", "ObjectDBX Classes", true, 0x401));
    add("IMAGE", cp("IMAGE", "AcDbRasterImage", "ISM", true, 127));
    add("IMAGEDEF", cp("IMAGEDEF", "AcDbRasterImageDef", "ISM", false, 0));
    add("IMAGEDEF_REACTOR", cp("IMAGEDEF_REACTOR", "AcDbRasterImageDefReactor", "ISM", false, 1));
    add("RASTERVARIABLES", cp("RASTERVARIABLES", "AcDbRasterVariables", "ISM", false, 0));
    add("WIPEOUT", cp("WIPEOUT", "AcDbWipeout", "WipeOut", true, 127));
    add("WIPEOUTVARIABLES", cp("WIPEOUTVARIABLES", "AcDbWipeoutVariables", "WipeOut", false, 0));
    // mleader-write.js (loaded before nongraphic-write.js's own table)
    add("MULTILEADER", cp("MULTILEADER", "AcDbMLeader", "ACDB_MLEADER_CLASS", true, 1025));
    add("MLEADERSTYLE", cp("MLEADERSTYLE", "AcDbMLeaderStyle", "ACDB_MLEADERSTYLE_CLASS", false, 4095));
    // nongraphic-write.js
    add("DICTIONARYVAR", cp("DICTIONARYVAR", "AcDbDictionaryVar", "ObjectDBX Classes", false, 0));
    add("SCALE", cp("SCALE", "AcDbScale", "ObjectDBX Classes", false, 1153));
    add("SORTENTSTABLE", cp("SORTENTSTABLE", "AcDbSortentsTable", "ObjectDBX Classes", false, 0));
    add("IDBUFFER", cp("IDBUFFER", "AcDbIdBuffer", "ObjectDBX Classes", false, 0));
    add("LAYER_INDEX", cp("LAYER_INDEX", "AcDbLayerIndex", "ObjectDBX Classes", false, 0));
    add("SPATIAL_FILTER", cp("SPATIAL_FILTER", "AcDbSpatialFilter", "ObjectDBX Classes", false, 0));
    add("FIELDLIST", cp("FIELDLIST", "AcDbFieldList", "ObjectDBX Classes", false, 1152));
    add("PLOTSETTINGS", cp("PLOTSETTINGS", "AcDbPlotSettings", "ObjectDBX Classes", false, 0));
    // table-write.js
    add("ACAD_TABLE", cp("ACAD_TABLE", "AcDbTable", "ObjectDBX Classes", true, 1025));
    add("TABLESTYLE", cp("TABLESTYLE", "AcDbTableStyle", "ObjectDBX Classes", false, 4095));
    add("FIELD", cp("FIELD", "AcDbField", "ObjectDBX Classes", false, 1152));
    // solid-write.js
    add("LIGHT", cp("LIGHT", "AcDbLight", "SCENEOE", true, 1025));
    // surface-write.js (surface-layout.js SURFACE_CLASSES)
    add("SURFACE", cp("SURFACE", "AcDbSurface", "ObjectDBX Classes", true, 4095));
    add("PLANESURFACE", cp("PLANESURFACE", "AcDbPlaneSurface", "ObjectDBX Classes", true, 4095));
    add("EXTRUDEDSURFACE", cp("EXTRUDEDSURFACE", "AcDbExtrudedSurface", "ObjectDBX Classes", true, 4093));
    add("LOFTEDSURFACE", cp("LOFTEDSURFACE", "AcDbLoftedSurface", "ObjectDBX Classes", true, 4093));
    add("REVOLVEDSURFACE", cp("REVOLVEDSURFACE", "AcDbRevolvedSurface", "ObjectDBX Classes", true, 4093));
    add("SWEPTSURFACE", cp("SWEPTSURFACE", "AcDbSweptSurface", "ObjectDBX Classes", true, 4093));
    add("NURBSURFACE", cp("NURBSURFACE", "AcDbNurbSurface", "ObjectDBX Classes", true, 0));
    m
}

#[derive(Clone, Debug)]
pub struct ClassEntry {
    pub props: Option<ClassProps>,
    pub num: f64,
    pub count: u32,
}
/// write.js classNum and the CLASSED registry (per write: the JS table is module-global, see the report notes).
#[derive(Clone, Debug, Default)]
pub struct Classes {
    pub classed: HashMap<String, ClassProps>,
    pub list: Vec<ClassEntry>,
    nums: HashMap<String, usize>,
}
impl Classes {
    pub fn new() -> Self {
        Classes { classed: classed_init(), list: Vec::new(), nums: HashMap::new() }
    }
    pub fn class_num(&mut self, t: &str) -> f64 {
        let i = match self.nums.get(t) {
            Some(i) => *i,
            None => {
                let i = self.list.len();
                self.list.push(ClassEntry { props: self.classed.get(t).cloned(), num: 500.0 + usize_to_f64(i), count: 0 });
                self.nums.insert(t.to_string(), i);
                i
            }
        };
        match self.list.get_mut(i) {
            Some(c) => {
                c.count = c.count.saturating_add(1);
                c.num
            }
            None => 0.0,
        }
    }
    /// `if (!CLASSED[k]) CLASSED[k] = c`
    pub fn register(&mut self, k: &str, c: ClassProps) {
        self.classed.entry(k.to_string()).or_insert(c);
    }
    pub fn has(&self, k: &str) -> bool {
        self.classed.contains_key(k)
    }
}

// ---------------------------------------------------------------- records
/// One record (a JS record object): its plain fields plus what other modules attach.
#[derive(Clone, Debug, Default)]
pub struct Rec {
    pub o: Obj,
    pub enc: Enc,
    /// `blockRef`: Some(Some(i)) a block, Some(None) the key present but undefined.
    pub block_ref: Option<Option<usize>>,
    /// table-write.js data of ACAD_TABLE / TABLESTYLE / FIELD records.
    pub tx: Option<Box<crate::w_table::TableX>>,
}
/// The record's `encode` (custom encoders of the other modules); `Std` = the ENC table by type.
#[derive(Clone, Debug, Default)]
pub enum Enc {
    #[default]
    Std,
    Raw(Box<crate::w_passthrough::RawRec>),
    Mleader(Box<crate::w_mleader::MlRec>),
    MleaderStyle(Box<crate::w_mleader::MlsRec>),
    XrecordCopy(Box<crate::w_nongraphic::XrCopy>),
    FieldCopy(Box<crate::w_nongraphic::FieldCopy>),
    Light,
    Acis(Box<crate::w_solid::AcisRec>),
    Surface(Box<crate::w_surface::SurfRec>),
}
impl Rec {
    pub fn new(o: Obj) -> Self {
        Rec { o, ..Default::default() }
    }
    pub fn t(&self) -> &str {
        self.o.str("t").unwrap_or("")
    }
    pub fn h(&self) -> f64 {
        num(self.o.get("h"), 0.0)
    }
    pub fn g(&self, k: &str) -> Option<&V> {
        self.o.get(k)
    }
}

/// What encoders need at encode time (everything is allocated by then).
pub struct Env<'a> {
    pub src_map: &'a HashMap<String, f64>,
    pub block_by_name: &'a HashMap<String, usize>,
    pub block_hdr: &'a [f64],
    pub style_by_name: &'a HashMap<String, f64>,
    pub lt_by_name: &'a HashMap<String, f64>,
    /// model.classes by number (clsOf; nongraphic-write.js num_key)
    pub model_classes: &'a HashMap<u64, ClassProps>,
}
impl Env<'_> {
    /// `srcMap.get(hexKey(hex)) || 0`
    pub fn map(&self, hex: Option<&V>) -> f64 {
        hex_key(hex).and_then(|k| self.src_map.get(&k).copied()).unwrap_or(0.0)
    }
    pub fn map_str(&self, k: &str) -> f64 {
        self.src_map.get(k).copied().unwrap_or(0.0)
    }
    /// write.js styleRef
    pub fn style_ref(&self, name: Option<&V>) -> f64 {
        style_ref(self.style_by_name, self.src_map, name)
    }
    /// write.js ltypeRef(name).h || 0
    pub fn ltype_h(&self, name: Option<&V>) -> f64 {
        ltype_ref(self.lt_by_name, self.src_map, name).1
    }
    pub fn cls_of(&self, n: f64) -> Option<ClassProps> {
        self.model_classes.get(&crate::w_nongraphic::num_key(n)).cloned()
    }
}
pub fn style_ref(style_by_name: &HashMap<String, f64>, src_map: &HashMap<String, f64>, name: Option<&V>) -> f64 {
    let up = str_or(name, "Standard").to_uppercase();
    if let Some(h) = style_by_name.get(&up).copied().filter(|h| *h != 0.0) {
        return h;
    }
    if let Some(h) = hex_key(name).and_then(|k| src_map.get(&k).copied()).filter(|h| *h != 0.0) {
        return h;
    }
    style_by_name.get("STANDARD").copied().unwrap_or(0.0)
}
/// write.js ltypeRef → (flag, h)
pub fn ltype_ref(lt_by_name: &HashMap<String, f64>, src_map: &HashMap<String, f64>, name: Option<&V>) -> (u32, f64) {
    let Some(nv) = nn(name) else { return (0, 0.0) };
    let s = js_string(Some(nv));
    if s.eq_ignore_ascii_case("bylayer") {
        return (0, 0.0);
    }
    if s.eq_ignore_ascii_case("byblock") {
        return (1, 0.0);
    }
    let h = lt_by_name
        .get(&s.to_uppercase())
        .copied()
        .filter(|h| *h != 0.0)
        .or_else(|| hex_key(name).and_then(|k| src_map.get(&k).copied()).filter(|h| *h != 0.0));
    match h {
        Some(h) => (3, h),
        None => (0, 0.0),
    }
}

/// The encode context: version, the class registry, the environment.
pub struct Cx<'a> {
    pub version: &'static str,
    pub ver: u32,
    pub classes: Classes,
    pub env: Env<'a>,
}

// ---------------------------------------------------------------- framing (20.1, 20.2)
pub fn begin(w: &mut BitWriter, ty: f64, rec: &Rec) {
    w.ot(ty);
    if !w.r2010 {
        w.size_at = w.bit;
        w.rl_u(0);
    }
    w.hd(0, rec.h());
    write_eed(w, rec.g("eed"));
}
/// Finish: patch the bit size, add the handle stream, frame with MS (+ R2010 handle-stream size) and CRC.
pub fn finish(w: &mut BitWriter) -> Vec<u8> {
    let endbit = w.end_data();
    frame_object(w, endbit)
}
/// passthrough.js frameObject (finish without endData).
pub fn frame_object(w: &mut BitWriter, endbit: usize) -> Vec<u8> {
    if !w.r2010 {
        let at = w.size_at;
        w.patch_rl(at, crate::util::u32_of_usize(endbit));
    }
    w.append_hnd();
    let data = w.bytes();
    let mut out = ByteWriter::new(data.len().saturating_add(8));
    out.ms(usize_to_f64(data.len()));
    if w.r2010 {
        out.raw(&umc_bytes(usize_to_f64(data.len()) * 8.0 - usize_to_f64(endbit)));
    }
    out.raw(&data);
    let crc = crc16(0xC0C1, &out.buf, 0, out.buf.len());
    out.rc8(crate::util::u8_of(u32::from(crc & 0xFF)));
    out.rc8(crate::util::u8_of(u32::from(crc >> 8)));
    out.buf
}

/// Extended entity data (ch. 28): per application BS size, app handle (data stream), items.
pub fn write_eed(w: &mut BitWriter, eed: Option<&V>) {
    for e in arr(eed) {
        let b = eed_bytes(field(Some(e), "items"), w.r2007);
        if b.is_empty() {
            continue;
        }
        w.bs(usize_to_f64(b.len()));
        let app = crate::w_bits::handle_value(field(Some(e), "app"));
        w.hd(5, app);
        for x in b {
            w.rc8(x);
        }
    }
    w.bs(0.0);
}
fn hex_pairs(s: &str) -> Vec<u8> {
    // `s.match(/[0-9a-f]{2}/gi)`
    let mut out = Vec::new();
    let mut prev: Option<u32> = None;
    for c in s.chars() {
        match c.to_digit(16) {
            Some(d) => match prev {
                Some(p) => {
                    out.push(crate::util::u8_of((p << 4) | d));
                    prev = None;
                }
                None => prev = Some(d),
            },
            None => prev = None,
        }
    }
    out
}
fn eed_bytes(items: Option<&V>, r2007: bool) -> Vec<u8> {
    let mut o = ByteWriter::new(64);
    for it in arr(items) {
        let Some(pair) = it.as_arr() else { continue };
        let Some(V::Num(code)) = pair.first() else { continue };
        let code = *code;
        let v = pair.get(1);
        if code == 1000.0 {
            o.rc8(0);
            if r2007 {
                let s = clean_units(&units(&str_or(v, "")));
                let s = s.get(..s.len().min(255)).unwrap_or(&[]);
                o.rs(usize_to_f64(s.len()));
                for &c in s {
                    o.rs_i(i32::from(c));
                }
            } else {
                // Spec gap: the code page after the length byte is big-endian (00 1E for ANSI_1252).
                let b = encode1252(&str_or(v, ""));
                let b = b.get(..b.len().min(255)).unwrap_or(&[]);
                o.rc(usize_to_f64(b.len()));
                o.rc8(0);
                o.rc8(30);
                o.raw(b);
            }
        } else if code == 1002.0 {
            o.rc8(2);
            o.rc8(u8::from(matches!(v, Some(V::Str(s)) if s == "}")));
        } else if code == 1003.0 || code == 1005.0 {
            o.rc(code - 1000.0);
            let x = match v {
                Some(V::Num(n)) => crate::util::f64_to_u64_wrapping(*n),
                _ => {
                    let n = parse_int16(&js_string(v));
                    crate::util::f64_to_u64_wrapping(if n.is_nan() { 0.0 } else { n })
                }
            };
            o.raw(&x.to_be_bytes());
        } else if code == 1004.0 {
            let b = match v {
                Some(V::Bytes(b)) => b.clone(),
                Some(V::Str(s)) => hex_pairs(s),
                _ => Vec::new(),
            };
            o.rc8(4);
            o.rc(usize_to_f64(b.len().min(255)));
            o.raw(b.get(..b.len().min(255)).unwrap_or(&[]));
        } else if (1010.0..=1013.0).contains(&code) {
            o.rc(code - 1000.0);
            for (k, i) in [("x", 0usize), ("y", 1), ("z", 2)] {
                let c = nn(field(v, k)).or_else(|| nn(v.and_then(V::as_arr).and_then(|a| a.get(i))));
                o.rd(match c {
                    Some(x) => to_number(Some(x)),
                    None => 0.0,
                });
            }
        } else if (1040.0..=1042.0).contains(&code) {
            o.rc(code - 1000.0);
            o.rd(plus_or0(v));
        } else if code == 1070.0 {
            o.rc8(70);
            o.rs_i(int32(v));
        } else if code == 1071.0 {
            o.rc8(71);
            o.rl(f64::from(int32(v)));
        }
    }
    o.buf
}
/// `+v || 0`
pub fn plus_or0(v: Option<&V>) -> f64 {
    let n = to_number(v);
    if n == 0.0 || n.is_nan() { 0.0 } else { n }
}

// ---------------------------------------------------------------- common entity data (20.4.1, 20.4.2)
fn flag_of(r: &Rec, k: &str) -> f64 {
    match nn(field(r.g(k), "flag")) {
        Some(v) => to_number(Some(v)),
        None => 0.0,
    }
}
/// `(r.mode ?? 2)`
pub fn mode_of(r: &Rec) -> f64 {
    nn(r.g("mode")).map_or(2.0, |v| to_number(Some(v)))
}
/// transparency for ENC: `r.transparency` when not nullish (as `transparency | 0`)
pub fn transparency_of(r: &Rec) -> Option<f64> {
    nn(r.g("transparency")).map(|v| to_number(Some(v)))
}
pub fn entity_common(w: &mut BitWriter, r: &Rec) {
    w.b(false); // no proxy graphics
    entity_common_rest(w, r, false);
}
/// The part after the proxy-graphics bit (shared with passthrough.js rawEntityCommon, which writes hard pointers with
/// code 5 the same way).
pub fn entity_common_rest(w: &mut BitWriter, r: &Rec, raw: bool) {
    let mode = mode_of(r);
    w.bb(crate::util::u32_from_i32(to_i32(mode)));
    let reactors = arr(r.g("reactors"));
    w.bl(usize_to_f64(reactors.len()));
    if w.r2004 {
        w.b(!truthy(r.g("xdic")));
    }
    if w.r2013 {
        w.b(!raw && truthy(r.g("ds")));
    }
    // Spec gap: the "Nolinks" bit is written only up to R2000.
    let prev_undef = r.g("prev").is_none();
    if !w.r2004 {
        w.b(prev_undef);
    }
    let t = if w.r2004 { transparency_of(r) } else { None };
    w.enc(r.g("color"), t);
    w.bd(nn(r.g("ltscale")).map_or(1.0, |v| to_number(Some(v))));
    let lt_flag = flag_of(r, "ltype");
    let ps_flag = flag_of(r, "plotstyle");
    let mat_flag = flag_of(r, "material");
    w.bb(crate::util::u32_from_i32(to_i32(lt_flag)));
    w.bb(crate::util::u32_from_i32(to_i32(ps_flag)));
    if w.r2007 {
        w.bb(crate::util::u32_from_i32(to_i32(mat_flag)));
        w.rc(nn(r.g("shadow")).map_or(0.0, |v| to_number(Some(v))));
    }
    if w.r2010 {
        w.b(false);
        w.b(false);
        w.b(false);
    }
    w.bs(if truthy(r.g("invisible")) { 1.0 } else { 0.0 });
    w.rc(nn(r.g("lw")).map_or(29.0, |v| to_number(Some(v))));
    // handles
    if mode == 0.0 && matches!(nn(r.g("mode")), None | Some(V::Num(_))) {
        w.hv(SOFT_PTR, r.g("owner"));
    }
    for x in reactors {
        w.hv(SOFT_PTR, Some(x));
    }
    if !w.r2004 || truthy(r.g("xdic")) {
        w.hv(HARD_OWN, r.g("xdic"));
    }
    if !w.r2004 && !prev_undef {
        w.hv(SOFT_PTR, r.g("prev"));
        w.hv(SOFT_PTR, r.g("next"));
    }
    w.hv(HARD_PTR, r.g("layer"));
    if lt_flag == 3.0 {
        w.hv(HARD_PTR, field(r.g("ltype"), "h"));
    }
    if w.r2007 && mat_flag == 3.0 {
        w.hv(HARD_PTR, field(r.g("material"), "h"));
    }
    if ps_flag == 3.0 {
        w.hv(HARD_PTR, field(r.g("plotstyle"), "h"));
    }
}
/// Common non-entity data (20.1).
pub fn object_common(w: &mut BitWriter, r: &Rec) {
    let reactors = arr(r.g("reactors"));
    w.bl(usize_to_f64(reactors.len()));
    if w.r2004 {
        w.b(!truthy(r.g("xdic")));
    }
    if w.r2013 {
        w.b(false);
    }
    let code = nn(r.g("ownerCode")).map_or(f64::from(SOFT_PTR), |v| to_number(Some(v)));
    w.hv(crate::util::u32_from_i32(to_i32(code)), r.g("owner"));
    for x in reactors {
        w.hv(SOFT_PTR, Some(x));
    }
    if !w.r2004 || truthy(r.g("xdic")) {
        w.hv(HARD_OWN, r.g("xdic"));
    }
}
/// Table entry header. Spec gap: from R2007 the 64-flag and Xdep bits are gone — only the xref index BS follows the
/// name; every entry had "BS 0" here, which reads as 64-flag 1 and xdep 0.
pub fn table_entry(w: &mut BitWriter, name: Option<&V>, flags: i32) {
    w.tv(&str_or(name, ""));
    if !w.r2007 {
        w.b(flags & 64 != 0);
        w.bs(0.0);
        w.b(flags & 16 != 0);
        return;
    }
    w.b(true);
    w.b(flags & 16 != 0);
}

/// `(+a || 0) % 2π`, made positive.
pub fn rad(a: Option<&V>) -> f64 {
    let t = 2.0 * std::f64::consts::PI;
    let mut r = plus_or0(a) % t;
    if r < 0.0 {
        r += t;
    }
    r
}
/// `{x: num(p?.x), y: num(p?.y), z: num(p?.z)}`
pub fn pt(p: Option<&V>) -> (f64, f64, f64) {
    (num(field(p, "x"), 0.0), num(field(p, "y"), 0.0), num(field(p, "z"), 0.0))
}
pub fn is_default_extrusion(e: Option<&V>) -> bool {
    !truthy(e) || (num(field(e, "x"), 0.0) == 0.0 && num(field(e, "y"), 0.0) == 0.0 && num(field(e, "z"), 1.0) == 1.0)
}
pub fn bex(w: &mut BitWriter, e: Option<&V>) {
    if is_default_extrusion(e) {
        w.be(0.0, 0.0, 1.0);
    } else {
        w.be(num(field(e, "x"), 0.0), num(field(e, "y"), 0.0), num(field(e, "z"), 1.0));
    }
}
pub fn ext3(w: &mut BitWriter, e: Option<&V>) {
    if truthy(e) {
        w.bd3(num(field(e, "x"), 0.0), num(field(e, "y"), 0.0), num(field(e, "z"), 1.0));
    } else {
        w.bd3(0.0, 0.0, 1.0);
    }
}
fn p3v(x: f64, y: f64, z: f64) -> V {
    crate::value::p3(x, y, z)
}
/// `w.P3(P(p))`
fn p3p(w: &mut BitWriter, p: Option<&V>) {
    let (x, y, z) = pt(p);
    w.p3n(x, y, z);
}
fn b01(v: Option<&V>) -> bool {
    truthy(v)
}
/// `x ?? d` as a number
pub fn nnum(v: Option<&V>, d: f64) -> f64 {
    nn(v).map_or(d, |x| to_number(Some(x)))
}
/// `x || d` as a number
pub fn ornum(v: Option<&V>, d: f64) -> f64 {
    if truthy(v) { to_number(v) } else { d }
}
fn items_of(r: &Rec) -> Vec<(V, V)> {
    arr(r.g("items"))
        .iter()
        .map(|it| {
            let a = it.as_arr().map(Vec::as_slice).unwrap_or(&[]);
            (a.first().cloned().unwrap_or(V::Null), a.get(1).cloned().unwrap_or(V::Null))
        })
        .collect()
}
fn hv_list(w: &mut BitWriter, code: u32, list: Option<&V>) {
    for h in arr(list) {
        w.hv(code, Some(h));
    }
}

// ---------------------------------------------------------------- DIMSTYLE variables
/// AutoCAD's imperial defaults (objects-write.js DIM_DEFAULTS).
pub fn dim_default(k: &str) -> Option<V> {
    Some(match k {
        "DIMPOST" | "DIMAPOST" | "DIMALTMZS" | "DIMMZS" => V::Str(String::new()),
        "DIMSCALE" | "DIMFXL" | "DIMTIH" | "DIMTOH" | "DIMLFAC" | "DIMTFAC" | "DIMTOLJ" => V::Num(1.0),
        "DIMASZ" | "DIMEXE" | "DIMTXT" => V::Num(0.18),
        "DIMEXO" => V::Num(0.0625),
        "DIMDLI" => V::Num(0.38),
        "DIMJOGANG" => V::Num(std::f64::consts::PI / 4.0),
        "DIMCEN" | "DIMGAP" => V::Num(0.09),
        "DIMALTF" => V::Num(25.4),
        "DIMALTD" | "DIMALTU" | "DIMALTTD" | "DIMLUNIT" => V::Num(2.0),
        "DIMDEC" | "DIMTDEC" => V::Num(4.0),
        "DIMDSEP" => V::Num(46.0),
        "DIMATFIT" => V::Num(3.0),
        "DIMALTMZF" | "DIMMZF" => V::Num(100.0),
        "DIMLWD" | "DIMLWE" => V::Num(-2.0),
        "DIMRND" | "DIMDLE" | "DIMTP" | "DIMTM" | "DIMTFILL" | "DIMTFILLCLR" | "DIMTOL" | "DIMLIM" | "DIMSE1" | "DIMSE2" | "DIMTAD"
        | "DIMZIN" | "DIMAZIN" | "DIMARCSYM" | "DIMTSZ" | "DIMTVP" | "DIMALTRND" | "DIMALT" | "DIMTOFL" | "DIMSAH" | "DIMTIX"
        | "DIMSOXD" | "DIMCLRD" | "DIMCLRE" | "DIMCLRT" | "DIMADEC" | "DIMAUNIT" | "DIMFRAC" | "DIMTMOVE" | "DIMJUST" | "DIMSD1"
        | "DIMSD2" | "DIMTZIN" | "DIMALTZ" | "DIMALTTZ" | "DIMUPT" | "DIMFXLON" | "DIMTXTDIRECTION" => V::Num(0.0),
        _ => return None,
    })
}
fn dim_vars(w: &mut BitWriter, vars: Option<&Obj>) {
    let g = |k: &str| -> Option<V> {
        match vars.and_then(|o| o.get(k)) {
            Some(v) => Some(v.clone()),
            None => dim_default(k),
        }
    };
    let clr = |k: &str| -> Option<V> {
        match g(k) {
            Some(V::Num(n)) => Some(V::Obj(obj! { "index" => n })),
            x => x,
        }
    };
    let gs = |k: &str| str_or(g(k).as_ref(), "");
    w.tv(&gs("DIMPOST"));
    w.tv(&gs("DIMAPOST"));
    for k in ["DIMSCALE", "DIMASZ", "DIMEXO", "DIMDLI", "DIMEXE", "DIMRND", "DIMDLE", "DIMTP", "DIMTM"] {
        w.bd(num(g(k).as_ref(), 0.0));
    }
    if w.r2007 {
        w.bd(num(g("DIMFXL").as_ref(), 0.0));
        w.bd(num(g("DIMJOGANG").as_ref(), 0.0));
        w.bs(int(g("DIMTFILL").as_ref()));
        w.cmc(clr("DIMTFILLCLR").as_ref());
    }
    for k in ["DIMTOL", "DIMLIM", "DIMTIH", "DIMTOH", "DIMSE1", "DIMSE2"] {
        w.b(truthy(g(k).as_ref()));
    }
    w.bs(int(g("DIMTAD").as_ref()));
    w.bs(int(g("DIMZIN").as_ref()));
    w.bs(int(g("DIMAZIN").as_ref()));
    if w.r2007 {
        w.bs(int(g("DIMARCSYM").as_ref()));
    }
    for k in ["DIMTXT", "DIMCEN", "DIMTSZ", "DIMALTF", "DIMLFAC", "DIMTVP", "DIMTFAC", "DIMGAP", "DIMALTRND"] {
        w.bd(num(g(k).as_ref(), 0.0));
    }
    w.b(truthy(g("DIMALT").as_ref()));
    w.bs(int(g("DIMALTD").as_ref()));
    for k in ["DIMTOFL", "DIMSAH", "DIMTIX", "DIMSOXD"] {
        w.b(truthy(g(k).as_ref()));
    }
    // Spec gap: the DIMSTYLE object's three colours are CmColor (CMC) like the header's, not plain BS, from R2004.
    for k in ["DIMCLRD", "DIMCLRE", "DIMCLRT"] {
        w.cmc(clr(k).as_ref());
    }
    for k in ["DIMADEC", "DIMDEC", "DIMTDEC", "DIMALTU", "DIMALTTD", "DIMAUNIT", "DIMFRAC", "DIMLUNIT", "DIMDSEP", "DIMTMOVE", "DIMJUST"] {
        w.bs(int(g(k).as_ref()));
    }
    w.b(truthy(g("DIMSD1").as_ref()));
    w.b(truthy(g("DIMSD2").as_ref()));
    for k in ["DIMTOLJ", "DIMTZIN", "DIMALTZ", "DIMALTTZ"] {
        w.bs(int(g(k).as_ref()));
    }
    w.b(truthy(g("DIMUPT").as_ref()));
    w.bs(int(g("DIMATFIT").as_ref()));
    if w.r2007 {
        w.b(truthy(g("DIMFXLON").as_ref()));
    }
    if w.r2010 {
        w.b(truthy(g("DIMTXTDIRECTION").as_ref()));
        w.bd(num(g("DIMALTMZF").as_ref(), 0.0));
        w.tv(&gs("DIMALTMZS"));
        w.bd(num(g("DIMMZF").as_ref(), 0.0));
        w.tv(&gs("DIMMZS"));
    }
}

// ---------------------------------------------------------------- XRECORD data
/// objects-read.js groupType with the JS comparisons on a number.
pub fn group_type(c: f64) -> Option<&'static str> {
    let r = |a: f64, b: f64| c >= a && c <= b;
    Some(if r(0.0, 9.0) {
        "str"
    } else if r(10.0, 39.0) {
        "pt3"
    } else if r(40.0, 59.0) {
        "dbl"
    } else if r(60.0, 79.0) {
        "i16"
    } else if r(90.0, 99.0) {
        "i32"
    } else if c == 100.0 || c == 101.0 || c == 102.0 {
        "str"
    } else if c == 105.0 {
        "hnd"
    } else if r(110.0, 149.0) {
        if c <= 139.0 { "pt3" } else { "dbl" }
    } else if r(160.0, 169.0) {
        "i64"
    } else if r(170.0, 179.0) {
        "i16"
    } else if r(210.0, 239.0) {
        "pt3"
    } else if r(270.0, 279.0) {
        "i16"
    } else if r(280.0, 289.0) {
        "i8"
    } else if r(290.0, 299.0) {
        "bool"
    } else if r(300.0, 309.0) {
        "str"
    } else if r(310.0, 319.0) {
        "bin"
    } else if r(320.0, 369.0) {
        "hnd"
    } else if r(370.0, 389.0) {
        "i16"
    } else if r(390.0, 399.0) {
        "hnd"
    } else if r(400.0, 409.0) {
        "i16"
    } else if r(410.0, 419.0) {
        "str"
    } else if r(420.0, 429.0) {
        "i32"
    } else if r(430.0, 439.0) {
        "str"
    } else if r(440.0, 459.0) {
        "i32"
    } else if r(460.0, 469.0) {
        "dbl"
    } else if r(470.0, 479.0) {
        "str"
    } else if r(480.0, 481.0) {
        "hnd"
    } else if c == 999.0 || r(1000.0, 1009.0) {
        "str"
    } else if r(1010.0, 1059.0) {
        if c <= 1039.0 { "pt3" } else { "dbl" }
    } else if r(1060.0, 1070.0) {
        "i16"
    } else if c == 1071.0 {
        "i32"
    } else {
        return None;
    })
}
/// `BigInt('0x' + hex)` taken to 64 bits (an invalid digit, where the JS would throw, ends the number).
fn hex_u64(s: &str) -> u64 {
    let t = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    let mut v: u64 = 0;
    for c in t.chars() {
        match c.to_digit(16) {
            Some(d) => v = (v << 4) | u64::from(d),
            None => break,
        }
    }
    v
}
/// XRECORD data bytes (20.4.104), every group code the way the reader decodes it. Spec gap: binary chunks (310–319)
/// are RC length + bytes, 280–289 one byte, 290–299 one byte, handles 8 bytes little-endian.
pub fn xrecord_bytes(items: &[V], r2007: bool) -> Vec<u8> {
    let mut o = ByteWriter::new(256);
    for it in items {
        let Some(pair) = it.as_arr() else { continue };
        let code = to_number(pair.first());
        let v = pair.get(1);
        let Some(t) = group_type(code) else { continue };
        o.rs(code);
        match t {
            "str" => {
                if r2007 {
                    let st = clean_units(&units(&str_or(v, "")));
                    o.rs(usize_to_f64(st.len()));
                    for &c in &st {
                        o.rs_i(i32::from(c));
                    }
                } else {
                    let b = encode1252(&str_or(v, ""));
                    o.rs(usize_to_f64(b.len()));
                    o.rc8(30);
                    o.raw(&b);
                }
            }
            "pt3" => {
                o.rd(plus_or0(field(v, "x")));
                o.rd(plus_or0(field(v, "y")));
                o.rd(plus_or0(field(v, "z")));
            }
            "dbl" => o.rd(plus_or0(v)),
            "i8" | "bool" => o.rc(f64::from(int32(v))),
            "i16" => o.rs_i(int32(v)),
            "i32" => o.rl(f64::from(int32(v))),
            "i64" => o.rll_u(crate::util::f64_to_u64_wrapping(plus_or0(v).trunc())),
            "hnd" => {
                let s = if truthy(v) { js_string(v) } else { "0".to_string() };
                o.rll_u(hex_u64(&s));
            }
            "bin" => {
                let hex: Vec<char> = (if truthy(v) { js_string(v) } else { String::new() }).chars().collect();
                let n = (hex.len() >> 1).min(255);
                o.rc(usize_to_f64(n));
                for i in 0..n {
                    let pair: String = hex.iter().skip(i.saturating_mul(2)).take(2).collect();
                    o.rc(parse_int16(&pair));
                }
            }
            _ => {}
        }
    }
    o.buf
}

// ---------------------------------------------------------------- encoders (the ENC table)
fn control(w: &mut BitWriter, r: &Rec) {
    object_common(w, r);
    match nn(r.g("count")) {
        Some(c) => w.bl(to_number(Some(c))),
        None => w.bl(usize_to_f64(arr(r.g("entries")).len())),
    }
    // Spec gap (our R2000 writer): the dimension-style table has one more RC after its entry count.
    if r.t() == "DIMSTYLE_CONTROL" {
        w.rc8(0);
    }
    hv_list(w, SOFT_OWN, r.g("entries"));
    hv_list(w, HARD_OWN, r.g("hardEntries"));
}

fn block_header(w: &mut BitWriter, r: &Rec) {
    object_common(w, r);
    table_entry(w, r.g("name"), if truthy(r.g("xdep")) { 16 } else { 0 });
    w.b(b01(r.g("anonymous")));
    w.b(b01(r.g("hasAttrs")));
    let is_xref = b01(r.g("isXref"));
    let overlaid = b01(r.g("overlaid"));
    w.b(is_xref);
    w.b(overlaid);
    w.b(false); // loaded
    let owned = arr(r.g("owned"));
    // Spec gap (as objects-read.js reads it): xref and overlay records have no owned object count
    if w.r2004 && !is_xref && !overlaid {
        w.bl(usize_to_f64(owned.len()));
    }
    w.p3(r.g("base"));
    w.tv(&if truthy(r.g("xrefPath")) { js_string(r.g("xrefPath")) } else { String::new() });
    // R2000+: one non-zero RC per INSERT handle, then 0. Spec gap: AutoCAD 2004+ files carry it the same way.
    let inserts = arr(r.g("inserts"));
    for _ in inserts {
        w.rc8(1);
    }
    w.rc8(0);
    w.tv(&if truthy(r.g("description")) { js_string(r.g("description")) } else { String::new() });
    match r.g("preview") {
        Some(V::Bytes(pv)) => {
            w.bl(usize_to_f64(pv.len()));
            for &b in pv {
                w.rc8(b);
            }
        }
        _ => w.bl(0.0),
    }
    if w.r2007 {
        w.bs(nnum(r.g("insertUnits"), 0.0));
        w.b(!matches!(r.g("explodable"), Some(V::Bool(false))));
        w.rc8(if matches!(r.g("scalable"), Some(V::Bool(false))) { 0 } else { 1 });
    }
    w.h(HARD_PTR, 0.0); // xref block (NULL)
    w.hv(HARD_OWN, r.g("block"));
    if !w.r2004 {
        if !is_xref && !overlaid {
            w.hv(SOFT_PTR, owned.first().filter(|v| v.truthy()));
            w.hv(SOFT_PTR, owned.last().filter(|v| v.truthy()));
        }
    } else if !is_xref && !overlaid {
        for h in owned {
            w.hv(HARD_OWN, Some(h));
        }
    }
    w.hv(HARD_OWN, r.g("endblk"));
    for h in inserts {
        w.hv(SOFT_PTR, Some(h));
    }
    w.hv(HARD_PTR, r.g("layout"));
}

fn layer(w: &mut BitWriter, r: &Rec) {
    object_common(w, r);
    table_entry(w, r.g("name"), if truthy(r.g("xdep")) { 16 } else { 0 });
    let lw = nnum(r.g("lwIndex"), 31.0);
    let flags = (if b01(r.g("frozen")) { 1 } else { 0 })
        | (if b01(r.g("off")) { 2 } else { 0 })
        | (if b01(r.g("frozenNew")) { 4 } else { 0 })
        | (if b01(r.g("locked")) { 8 } else { 0 })
        | (if matches!(r.g("plot"), Some(V::Bool(false))) { 0 } else { 16 })
        | ((to_i32(lw) & 31) << 5);
    w.bs(f64::from(flags));
    // Spec gap: "off" is the flag bit above; the colour index itself stays positive.
    let c = r.g("color");
    let rgb_undef = field(c, "rgb").is_none();
    if truthy(c) && rgb_undef {
        let idx = nnum(field(c, "index"), 7.0).abs();
        w.cmc(Some(&V::Obj(obj! { "index" => idx })));
    } else {
        w.cmc(c);
    }
    w.hv(HARD_PTR, r.g("xrefBlock"));
    w.hv(HARD_PTR, r.g("plotstyle"));
    if w.r2007 {
        w.hv(HARD_PTR, r.g("material"));
    }
    w.hv(HARD_PTR, r.g("ltype"));
    if w.r2013 {
        w.h(HARD_PTR, 0.0);
    }
}

fn style(w: &mut BitWriter, r: &Rec) {
    object_common(w, r);
    table_entry(w, r.g("name"), 0);
    w.b(b01(r.g("vertical")));
    w.b(b01(r.g("shapeFile")));
    w.bd(num(r.g("height"), 0.0));
    w.bd(num(r.g("widthFactor"), 1.0));
    w.bd(num(r.g("oblique"), 0.0));
    w.rc(f64::from(int32(r.g("generation"))));
    w.bd(num(r.g("lastHeight"), 0.2));
    w.tv(&str_or(r.g("font"), "txt"));
    w.tv(&str_or(r.g("bigFont"), ""));
    w.h(HARD_PTR, 0.0);
}

fn ltype(w: &mut BitWriter, r: &Rec) {
    object_common(w, r);
    table_entry(w, r.g("name"), 0);
    w.tv(&if truthy(r.g("description")) { js_string(r.g("description")) } else { String::new() });
    let els = arr(r.g("elements"));
    let sum = els.iter().fold(0.0, |s, e| s + num(field(Some(e), "length"), 0.0).abs());
    w.bd(num(r.g("patternLength"), sum));
    w.rc8(65);
    w.rc(usize_to_f64(els.len()));
    let mut text_area = false;
    for e in els {
        let e = Some(e);
        w.bd(num(field(e, "length"), 0.0));
        w.bs(f64::from(int32(field(e, "shape"))));
        let off = field(e, "offset");
        w.rd(num(nn(field(off, "x")).or_else(|| field(e, "offsetX")), 0.0));
        w.rd(num(nn(field(off, "y")).or_else(|| field(e, "offsetY")), 0.0));
        w.bd(num(field(e, "scale"), 1.0));
        w.bd(num(field(e, "rotation"), 0.0));
        let fl = int32(field(e, "flags"));
        w.bs(f64::from(fl));
        if fl & 2 != 0 {
            text_area = true;
        }
    }
    if !w.r2007 {
        for _ in 0..256 {
            w.rc8(0);
        }
    } else if text_area {
        for _ in 0..512 {
            w.rc8(0);
        }
    }
    w.h(HARD_PTR, 0.0);
    for e in els {
        w.hv(HARD_PTR, field(Some(e), "styleH"));
    }
}

fn ucs(w: &mut BitWriter, r: &Rec) {
    object_common(w, r);
    table_entry(w, r.g("name"), 0);
    let o = pt(r.g("origin"));
    let x = pt(r.g("xAxis"));
    let y = pt(r.g("yAxis"));
    w.bd3(o.0, o.1, o.2);
    w.bd3(x.0, x.1, x.2);
    w.bd3(y.0, y.1, y.2);
    w.bd(num(r.g("elevation"), 0.0));
    w.bs(0.0);
    w.bs(0.0);
    w.h(HARD_PTR, 0.0);
    w.h(HARD_PTR, 0.0);
    w.h(HARD_PTR, 0.0);
}

fn vport(w: &mut BitWriter, r: &Rec) {
    object_common(w, r);
    table_entry(w, r.g("name"), 0);
    let vh = num(r.g("viewHeight"), 1.0);
    let asp = num(r.g("aspect"), 1.5);
    w.bd(vh);
    w.bd(vh * asp);
    w.rd2(num(field(r.g("center"), "x"), 0.0), num(field(r.g("center"), "y"), 0.0));
    w.p3(r.g("target"));
    let dir = r.g("direction").filter(|v| v.truthy()).cloned().unwrap_or_else(|| p3v(0.0, 0.0, 1.0));
    ext3(w, Some(&dir));
    w.bd(num(r.g("twist"), 0.0));
    w.bd(num(r.g("lens"), 50.0));
    w.bd(0.0);
    w.bd(0.0);
    w.bits(crate::util::to_u32(nnum(r.g("viewMode"), 0.0)), 4);
    w.rc8(0); // render mode
    if w.r2007 {
        w.b(true);
        w.rc8(1);
        w.bd(0.0);
        w.bd(0.0);
        w.cmc(Some(&V::Obj(obj! { "index" => 250 })));
    }
    w.rd2(num(field(r.g("lowerLeft"), "x"), 0.0), num(field(r.g("lowerLeft"), "y"), 0.0));
    w.rd2(num(field(r.g("upperRight"), "x"), 1.0), num(field(r.g("upperRight"), "y"), 1.0));
    w.b(false);
    w.bs(1000.0);
    w.b(true);
    w.bits(0b11, 2);
    w.b(false);
    w.rd2(num(field(r.g("grid"), "x"), 10.0), num(field(r.g("grid"), "y"), 10.0));
    w.b(false);
    w.b(false);
    w.bs(0.0);
    w.bd(0.0);
    w.rd2(0.0, 0.0);
    w.rd2(num(field(r.g("snap"), "x"), 10.0), num(field(r.g("snap"), "y"), 10.0));
    w.b(false);
    w.b(true);
    w.bd3(0.0, 0.0, 0.0);
    w.bd3(1.0, 0.0, 0.0);
    w.bd3(0.0, 1.0, 0.0);
    w.bd(0.0);
    w.bs(0.0);
    if w.r2007 {
        w.bs(7.0);
        w.bs(5.0);
    }
    w.h(HARD_PTR, 0.0);
    if w.r2007 {
        w.h(SOFT_PTR, 0.0);
        w.h(HARD_PTR, 0.0);
        w.hv(HARD_OWN, r.g("sun"));
    }
    w.h(HARD_PTR, 0.0);
    w.h(HARD_PTR, 0.0);
}

fn view(w: &mut BitWriter, r: &Rec) {
    object_common(w, r);
    table_entry(w, r.g("name"), int32(r.g("flags")) & 64);
    w.bd(num(r.g("height"), 1.0));
    w.bd(num(r.g("width"), 1.0));
    w.rd2(num(field(r.g("center"), "x"), 0.0), num(field(r.g("center"), "y"), 0.0));
    p3p(w, r.g("target"));
    let dir = r.g("direction").filter(|v| v.truthy()).cloned().unwrap_or_else(|| p3v(0.0, 0.0, 1.0));
    ext3(w, Some(&dir));
    w.bd(num(r.g("twist"), 0.0));
    w.bd(num(r.g("lensLength"), 50.0));
    w.bd(num(r.g("frontClip"), 0.0));
    w.bd(num(r.g("backClip"), 0.0));
    w.bits(crate::util::u32_from_i32(int32(r.g("viewMode")) & 15), 4);
    w.rc(f64::from(int32(r.g("renderMode"))));
    if w.r2007 {
        w.b(!matches!(r.g("useDefaultLights"), Some(V::Bool(false))));
        w.rc(nnum(r.g("defaultLightingType"), 1.0));
        w.bd(num(r.g("brightness"), 0.0));
        w.bd(num(r.g("contrast"), 0.0));
        let amb = r.g("ambientColor").filter(|v| v.truthy()).cloned().unwrap_or_else(|| V::Obj(obj! { "index" => 250 }));
        w.cmc(Some(&amb));
    }
    w.b(b01(r.g("paperSpace")));
    let assoc = b01(r.g("associatedUcs"));
    w.b(assoc);
    if assoc {
        p3p(w, r.g("ucsOrigin"));
        if truthy(r.g("ucsXAxis")) {
            p3p(w, r.g("ucsXAxis"));
        } else {
            w.p3n(1.0, 0.0, 0.0);
        }
        if truthy(r.g("ucsYAxis")) {
            p3p(w, r.g("ucsYAxis"));
        } else {
            w.p3n(0.0, 1.0, 0.0);
        }
        w.bd(num(r.g("ucsElevation"), 0.0));
        w.bs(f64::from(int32(r.g("ucsOrthoType"))));
    }
    if w.r2007 {
        w.b(b01(r.g("cameraPlottable")));
    }
    w.h(HARD_PTR, 0.0);
    if w.r2007 {
        w.h(SOFT_PTR, 0.0);
        w.h(HARD_PTR, 0.0);
        w.h(HARD_OWN, 0.0);
    }
    if assoc {
        w.h(HARD_PTR, 0.0);
        w.h(HARD_PTR, 0.0);
    }
    if w.r2007 {
        w.h(SOFT_PTR, 0.0);
    }
}

fn appid(w: &mut BitWriter, r: &Rec) {
    object_common(w, r);
    table_entry(w, r.g("name"), 0);
    w.rc8(0);
    w.h(HARD_PTR, 0.0);
}

fn dimstyle(w: &mut BitWriter, r: &Rec) {
    object_common(w, r);
    table_entry(w, r.g("name"), 0);
    let vars = r.g("vars").and_then(V::as_obj);
    dim_vars(w, vars);
    w.bs(nnum(vars.and_then(|o| o.get("DIMLWD")), -2.0));
    w.bs(nnum(vars.and_then(|o| o.get("DIMLWE")), -2.0));
    w.b(false);
    w.h(HARD_PTR, 0.0);
    w.hv(HARD_PTR, r.g("txsty"));
    for k in ["ldrblk", "blk", "blk1", "blk2"] {
        w.hv(HARD_PTR, r.g(k));
    }
    if w.r2007 {
        for k in ["dimltype", "ltex1", "ltex2"] {
            w.hv(HARD_PTR, r.g(k));
        }
    }
}

pub fn dictionary(w: &mut BitWriter, r: &Rec) {
    object_common(w, r);
    let items = items_of(r);
    w.bl(usize_to_f64(items.len()));
    w.bs(nnum(r.g("cloning"), 1.0));
    let hard = truthy(r.g("hardOwner"));
    w.rc8(u8::from(hard));
    for (name, _) in &items {
        w.tv(&str_or(Some(name), ""));
    }
    for (_, h) in &items {
        w.hv(if hard { HARD_OWN } else { SOFT_OWN }, Some(h));
    }
}

fn xrecord(w: &mut BitWriter, r: &Rec) {
    object_common(w, r);
    let b = xrecord_bytes(arr(r.g("data")), w.r2007);
    w.bl(usize_to_f64(b.len()));
    for x in b {
        w.rc8(x);
    }
    w.bs(nnum(r.g("cloning"), 1.0));
    hv_list(w, SOFT_PTR, r.g("objids"));
}

fn mlinestyle(w: &mut BitWriter, r: &Rec) {
    object_common(w, r);
    w.tv(&if truthy(r.g("name")) { js_string(r.g("name")) } else { "Standard".into() });
    w.tv(&if truthy(r.g("description")) { js_string(r.g("description")) } else { String::new() });
    w.bs(0.0);
    let bylayer = V::Obj(obj! { "index" => 256 });
    w.cmc(Some(&bylayer));
    w.bd(std::f64::consts::PI / 2.0);
    w.bd(std::f64::consts::PI / 2.0);
    w.rc8(2);
    for off in [0.5, -0.5] {
        w.bd(off);
        w.cmc(Some(&bylayer));
        // Spec: from R2018 the element linetype is a handle (by ByLayer linetype), before that a BS index
        if w.r2018 {
            w.hv(HARD_PTR, r.g("ltByLayer"));
        } else {
            w.bs(32767.0);
        }
    }
}

fn layout(w: &mut BitWriter, r: &Rec) {
    object_common(w, r);
    let p = r.g("paper").filter(|v| v.truthy());
    let tab = truthy(r.g("tab"));
    let ps = |k: &str| field(p, k);
    w.tv(&if truthy(ps("pageSetup")) { js_string(ps("pageSetup")) } else { String::new() });
    w.tv(&if truthy(ps("printer")) { js_string(ps("printer")) } else { "none_device".into() });
    w.bs(nnum(r.g("plotFlags"), if tab { 688.0 } else { 1712.0 }));
    for k in ["left", "bottom", "right", "top"] {
        w.bd(num(ps(k), 6.35));
    }
    w.bd(num(ps("w"), 215.9));
    w.bd(num(ps("h"), 279.4));
    w.tv(&if truthy(ps("name")) { js_string(ps("name")) } else { "Letter_(8.50_x_11.00_Inches)".into() });
    w.bd2(0.0, 0.0);
    w.bs(nnum(ps("units"), if tab { 1.0 } else { 0.0 }));
    w.bs(nnum(ps("rotation"), 1.0));
    w.bs(nnum(ps("plotType"), if tab { 5.0 } else { 0.0 }));
    w.bd2(0.0, 0.0);
    w.bd2(0.0, 0.0);
    if !w.r2004 {
        w.t8(""); // plot view name (R13–R2000)
    }
    w.bd(1.0);
    w.bd(num(ps("drawingUnits"), 1.0));
    w.tv(&if truthy(ps("styleSheet")) { js_string(ps("styleSheet")) } else { String::new() });
    w.bs(nnum(ps("scaleType"), if tab { 16.0 } else { 0.0 }));
    w.bd(num(ps("scale"), 1.0));
    w.bd2(0.0, 0.0);
    if w.r2004 {
        w.bs(0.0);
        w.bs(2.0);
        w.bs(300.0);
    }
    w.tv(&str_or(r.g("name"), ""));
    w.bl(f64::from(int32(r.g("tab"))));
    w.bs(nnum(r.g("flags"), 1.0));
    w.bd3(0.0, 0.0, 0.0);
    let lim = r.g("limits");
    w.rd2(num(field(field(lim, "min"), "x"), 0.0), num(field(field(lim, "min"), "y"), 0.0));
    w.rd2(num(field(field(lim, "max"), "x"), 12.0), num(field(field(lim, "max"), "y"), 9.0));
    w.p3(r.g("insbase"));
    w.bd3(1.0, 0.0, 0.0);
    w.bd3(0.0, 1.0, 0.0);
    w.bd(0.0);
    w.bs(0.0);
    match r.g("extents").filter(|v| v.truthy()) {
        Some(e) => {
            w.p3(field(Some(e), "min"));
            w.p3(field(Some(e), "max"));
        }
        None => {
            w.p3n(1e20, 1e20, 1e20);
            w.p3n(-1e20, -1e20, -1e20);
        }
    }
    let vps = arr(r.g("viewports"));
    // Spec gap: the viewport count is a BL, not an RL.
    if w.r2004 {
        w.bl(usize_to_f64(vps.len()));
    }
    if w.r2004 {
        w.h(HARD_PTR, 0.0);
    }
    if w.r2007 {
        w.h(SOFT_PTR, 0.0);
    }
    w.hv(SOFT_PTR, r.g("block"));
    w.hv(SOFT_PTR, r.g("lastViewport"));
    w.h(HARD_PTR, 0.0);
    w.h(HARD_PTR, 0.0);
    if w.r2004 {
        for h in vps {
            w.hv(SOFT_PTR, Some(h));
        }
    }
}

fn line(w: &mut BitWriter, r: &Rec) {
    entity_common(w, r);
    let a = pt(r.g("start"));
    let b = pt(r.g("end"));
    let flat = a.2 == 0.0 && b.2 == 0.0;
    w.b(flat);
    w.rd(a.0);
    w.dd(b.0, a.0);
    w.rd(a.1);
    w.dd(b.1, a.1);
    if !flat {
        w.rd(a.2);
        w.dd(b.2, a.2);
    }
    w.bt(num(r.g("thickness"), 0.0));
    bex(w, r.g("extrusion"));
}
fn corners(w: &mut BitWriter, r: &Rec) {
    entity_common(w, r);
    w.bt(num(r.g("thickness"), 0.0));
    let pts = arr(r.g("points"));
    let elev = match nn(r.g("elevation")) {
        Some(v) => Some(v.clone()),
        None => pts.first().and_then(|p| field(Some(p), "z")).cloned(),
    };
    w.bd(num(elev.as_ref(), 0.0));
    for i in 0..4usize {
        let p = pts.get(i).filter(|p| p.truthy()).or_else(|| pts.last());
        w.rd2(num(field(p, "x"), 0.0), num(field(p, "y"), 0.0));
    }
    bex(w, r.g("extrusion"));
}
fn face3d(w: &mut BitWriter, r: &Rec) {
    entity_common(w, r);
    let pts = arr(r.g("points"));
    let p: Vec<(f64, f64, f64)> = (0..4usize).map(|i| pt(pts.get(i).filter(|p| p.truthy()).or_else(|| pts.last()))).collect();
    let flags = int32(r.g("invisibleEdges"));
    let zz = p.iter().all(|q| q.2 == 0.0 || q.2.is_nan());
    w.b(flags == 0);
    w.b(zz);
    let p0 = p.first().copied().unwrap_or_default();
    w.rd(p0.0);
    w.rd(p0.1);
    if !zz {
        w.rd(p0.2);
    }
    for i in 1..4usize {
        let a = p.get(i).copied().unwrap_or_default();
        let b = p.get(i.saturating_sub(1)).copied().unwrap_or_default();
        w.dd(a.0, b.0);
        w.dd(a.1, b.1);
        w.dd(a.2, b.2);
    }
    if flags != 0 {
        w.bs(f64::from(flags));
    }
}

/// TEXT data (20.4.3 R2000+), also the start of ATTRIB / ATTDEF.
fn text_data(w: &mut BitWriter, r: &Rec) {
    let ins = pt(r.g("insert"));
    let al = if truthy(r.g("align")) { Some(pt(r.g("align"))) } else { None };
    let ha = int32(r.g("hAlign"));
    let va = int32(r.g("vAlign"));
    let has_align = al.is_some() && (ha != 0 || va != 0);
    let elev = ins.2;
    let rot = num(r.g("rotation"), 0.0);
    let wf = num(r.g("widthFactor"), 1.0);
    let obl = num(r.g("oblique"), 0.0);
    let gn = int32(r.g("generation"));
    let z = |b: bool, bit: u32| if b { 0 } else { bit };
    w.rc(f64::from(
        z(elev != 0.0, 1)
            | z(has_align, 2)
            | z(obl != 0.0, 4)
            | z(rot != 0.0, 8)
            | (if wf != 1.0 { 0 } else { 0x10 })
            | z(gn != 0, 0x20)
            | z(ha != 0, 0x40)
            | z(va != 0, 0x80),
    ));
    if elev != 0.0 {
        w.rd(elev);
    }
    w.rd2(ins.0, ins.1);
    if let (true, Some(al)) = (has_align, al) {
        w.dd(al.0, ins.0);
        w.dd(al.1, ins.1);
    }
    bex(w, r.g("extrusion"));
    w.bt(num(r.g("thickness"), 0.0));
    if obl != 0.0 {
        w.rd(obl);
    }
    if rot != 0.0 {
        w.rd(rot);
    }
    w.rd(num(r.g("height"), 0.2));
    if wf != 1.0 {
        w.rd(wf);
    }
    w.tv(&str_or(r.g("value"), ""));
    if gn != 0 {
        w.bs(f64::from(gn));
    }
    if ha != 0 {
        w.bs(f64::from(ha));
    }
    if va != 0 {
        w.bs(f64::from(va));
    }
}
fn attrib_tail(w: &mut BitWriter, r: &Rec) {
    if w.r2010 {
        w.rc8(0);
    }
    if w.r2018 {
        w.rc8(1);
    }
    w.tv(&str_or(r.g("tag"), ""));
    w.bs(0.0);
    w.rc(f64::from(int32(r.g("flags"))));
    if w.r2007 {
        w.b(b01(r.g("lockPosition")));
    }
}

fn mtext(w: &mut BitWriter, r: &Rec) {
    entity_common(w, r);
    let ins = pt(r.g("insert"));
    let xa = if truthy(r.g("xAxis")) {
        pt(r.g("xAxis"))
    } else {
        let rot = num(r.g("rotation"), 0.0);
        (rot.cos(), rot.sin(), 0.0)
    };
    w.bd3(ins.0, ins.1, ins.2);
    ext3(w, r.g("extrusion"));
    w.bd3(xa.0, xa.1, xa.2);
    w.bd(num(r.g("width"), 0.0));
    if w.r2007 {
        w.bd(num(r.g("rectHeight"), 0.0));
    }
    w.bd(num(r.g("height"), 0.2));
    w.bs(ornum(r.g("attach"), 1.0));
    w.bs(ornum(r.g("direction"), 1.0));
    w.bd(num(r.g("extHeight"), 0.0));
    w.bd(num(r.g("extWidth"), 0.0));
    w.tv(&str_or(r.g("value"), ""));
    w.hv(HARD_PTR, r.g("style")); // STYLE comes before the R2018 application handle
    w.bs(ornum(r.g("lineSpacingStyle"), 1.0));
    w.bd(num(r.g("lineSpacing"), 1.0));
    w.b(false);
    if w.r2004 {
        // background mask (0.5 WL.8)
        let bg = int32(r.g("backgroundFlags"));
        w.bl(f64::from(bg));
        if bg & 1 != 0 || (w.r2018 && bg & 0x10 != 0) {
            w.bd(num(r.g("backgroundScale"), 1.5));
            let c = r.g("backgroundColor").filter(|v| v.truthy()).cloned().unwrap_or_else(|| V::Obj(obj! { "index" => 7 }));
            w.cmc(Some(&c));
            w.bl(f64::from(int32(r.g("backgroundTransparency"))));
        }
    }
    if w.r2018 {
        // Spec gap (filled from AutoCAD 2018 files): not annotative → class version 4, default flag, the redundant
        // geometry block and column type.
        w.b(true);
        w.bs(4.0);
        w.b(true);
        w.h(HARD_PTR, 0.0);
        w.bl(ornum(r.g("attach"), 1.0));
        w.bd3(xa.0, xa.1, xa.2);
        w.bd3(ins.0, ins.1, ins.2);
        w.bd(num(r.g("width"), 0.0));
        w.bd(num(r.g("rectHeight"), 0.0));
        w.bd(num(r.g("extWidth"), 0.0));
        w.bd(num(r.g("extHeight"), 0.0));
        w.bs(0.0);
    }
}

pub fn insert_scale(w: &mut BitWriter, s: Option<&V>) {
    let sx = num(field(s, "x"), 1.0);
    let sy = num(field(s, "y"), 1.0);
    let sz = num(field(s, "z"), 1.0);
    if sx == 1.0 && sy == 1.0 && sz == 1.0 {
        w.bb(3);
    } else if sx == sy && sx == sz {
        w.bb(2);
        w.rd(sx);
    } else if sx == 1.0 {
        w.bb(1);
        w.dd(sy, 1.0);
        w.dd(sz, 1.0);
    } else {
        w.bb(0);
        w.rd(sx);
        w.dd(sy, sx);
        w.dd(sz, sx);
    }
}
fn insert_tail(w: &mut BitWriter, r: &Rec) {
    let atts = arr(r.g("attribs"));
    if !atts.is_empty() {
        if !w.r2004 {
            w.hv(SOFT_PTR, atts.first());
            w.hv(SOFT_PTR, atts.last());
        } else {
            for h in atts {
                w.hv(HARD_OWN, Some(h));
            }
        }
        w.hv(HARD_OWN, r.g("seqend"));
    }
}
fn insert(w: &mut BitWriter, r: &Rec, minsert: bool) {
    entity_common(w, r);
    w.p3(r.g("insert"));
    insert_scale(w, r.g("scale"));
    w.bd(num(r.g("rotation"), 0.0));
    ext3(w, r.g("extrusion"));
    let n = arr(r.g("attribs")).len();
    w.b(n > 0);
    if w.r2004 && n > 0 {
        w.bl(usize_to_f64(n));
    }
    if minsert {
        w.bs(ornum(r.g("cols"), 1.0));
        w.bs(ornum(r.g("rows"), 1.0));
        w.bd(num(r.g("colSpacing"), 0.0));
        w.bd(num(r.g("rowSpacing"), 0.0));
    }
    w.hv(HARD_PTR, r.g("block"));
    insert_tail(w, r);
}

fn poly_owned(w: &mut BitWriter, r: &Rec) {
    let vs = arr(r.g("vertices"));
    if w.r2004 {
        for h in vs {
            w.hv(HARD_OWN, Some(h));
        }
    } else {
        w.hv(SOFT_PTR, vs.first().filter(|v| v.truthy()));
        w.hv(SOFT_PTR, vs.last().filter(|v| v.truthy()));
    }
    w.hv(HARD_OWN, r.g("seqend"));
}
fn n_verts(r: &Rec) -> f64 {
    usize_to_f64(arr(r.g("vertices")).len())
}

fn lwpolyline(w: &mut BitWriter, r: &Rec) {
    entity_common(w, r);
    let v = arr(r.g("vertices"));
    let bulge_of = |p: &V| num(field(Some(p), "bulge"), 0.0);
    let bulges: Option<Vec<f64>> = if v.iter().any(|p| bulge_of(p) != 0.0) { Some(v.iter().map(bulge_of).collect()) } else { None };
    let widths = !truthy(r.g("constantWidth"))
        && v.iter().any(|p| num(field(Some(p), "startWidth"), 0.0) != 0.0 || num(field(Some(p), "endWidth"), 0.0) != 0.0);
    let ids: Option<Vec<i32>> = if w.r2010 && v.iter().any(|p| truthy(field(Some(p), "id"))) {
        Some(v.iter().map(|p| int32(field(Some(p), "id"))).collect())
    } else {
        None
    };
    let elev = num(r.g("elevation"), 0.0);
    let th = num(r.g("thickness"), 0.0);
    let cw = num(r.g("constantWidth"), 0.0);
    let ex = !is_default_extrusion(r.g("extrusion"));
    let flags = (if ex { 1 } else { 0 })
        | (if th != 0.0 { 2 } else { 0 })
        | (if cw != 0.0 { 4 } else { 0 })
        | (if elev != 0.0 { 8 } else { 0 })
        | (if bulges.is_some() { 16 } else { 0 })
        | (if widths { 32 } else { 0 })
        | (if truthy(r.g("plinegen")) { 256 } else { 0 })
        | (if truthy(r.g("closed")) { 512 } else { 0 })
        | (if ids.is_some() { 1024 } else { 0 });
    w.bs(f64::from(flags));
    if cw != 0.0 {
        w.bd(cw);
    }
    if elev != 0.0 {
        w.bd(elev);
    }
    if th != 0.0 {
        w.bd(th);
    }
    if ex {
        let e = r.g("extrusion");
        w.bd3(num(field(e, "x"), 0.0), num(field(e, "y"), 0.0), num(field(e, "z"), 1.0));
    }
    let n = usize_to_f64(v.len());
    w.bl(n);
    if bulges.is_some() {
        w.bl(n);
    }
    if ids.is_some() {
        w.bl(n);
    }
    if widths {
        w.bl(n);
    }
    let xy = |p: Option<&V>| (num(field(p, "x"), 0.0), num(field(p, "y"), 0.0));
    let p0 = xy(v.first());
    w.rd2(p0.0, p0.1);
    for k in 1..v.len() {
        let a = xy(v.get(k));
        let b = xy(v.get(k.saturating_sub(1)));
        w.dd(a.0, b.0);
        w.dd(a.1, b.1);
    }
    if let Some(bs) = bulges {
        for b in bs {
            w.bd(b);
        }
    }
    if let Some(ids) = ids {
        for i in ids {
            w.bl(f64::from(i));
        }
    }
    if widths {
        for p in v {
            w.bd(num(field(Some(p), "startWidth"), 0.0));
            w.bd(num(field(Some(p), "endWidth"), 0.0));
        }
    }
}

fn spline(w: &mut BitWriter, r: &Rec) {
    entity_common(w, r);
    let fit = arr(r.g("fit"));
    let ctrl = arr(r.g("control"));
    let knots = arr(r.g("knots"));
    let use_fit = fit.len() >= 2 && ctrl.is_empty();
    let fl = int32(r.g("flags"));
    let closed = fl & 1 != 0;
    if w.r2013 {
        // Spec gap (AutoCAD 2013 files): the scenario is written as 1 even for fit-point splines.
        w.bl(1.0);
        w.bl(if use_fit {
            f64::from(9 | if closed { 4 } else { 0 })
        } else if closed {
            4.0
        } else {
            0.0
        });
        let kp = r.g("knotParam");
        let kpn = match kp {
            Some(V::Num(n)) => *n,
            _ => to_number(kp),
        };
        w.bl(if use_fit { if (0.0..=2.0).contains(&kpn) { kpn } else { 0.0 } } else { 15.0 });
    } else {
        w.bl(if use_fit { 2.0 } else { 1.0 });
    }
    w.bl(ornum(r.g("degree"), 3.0));
    let weights_arr = arr(r.g("weights"));
    let weights = ctrl.iter().any(|p| {
        let wv = field(Some(p), "w");
        wv.is_some() && !matches!(wv, Some(V::Num(n)) if *n == 1.0)
    }) || weights_arr.iter().any(|x| !matches!(x, V::Num(n) if *n == 1.0));
    if use_fit {
        w.bd(num(r.g("fitTolerance"), 0.0));
        w.p3(r.g("startTangent"));
        w.p3(r.g("endTangent"));
        w.bl(usize_to_f64(fit.len()));
        for p in fit {
            w.p3(Some(p));
        }
    } else {
        w.b(weights);
        w.b(closed);
        w.b(fl & 2 != 0);
        w.bd(num(r.g("knotTolerance"), 1e-10));
        w.bd(num(r.g("controlTolerance"), 1e-10));
        w.bl(usize_to_f64(knots.len()));
        w.bl(usize_to_f64(ctrl.len()));
        w.b(weights);
        for k in knots {
            w.bd(num(Some(k), 0.0));
        }
        for (i, p) in ctrl.iter().enumerate() {
            w.p3(Some(p));
            if weights {
                let wv = nn(field(Some(p), "w")).or_else(|| weights_arr.get(i));
                w.bd(num(wv, 1.0));
            }
        }
    }
}

fn hatch(w: &mut BitWriter, r: &Rec) {
    entity_common(w, r);
    if w.r2004 {
        let g = r.g("gradient").filter(|v| v.truthy());
        w.bl(if g.is_some() { 1.0 } else { 0.0 });
        w.bl(0.0);
        w.bd(num(field(g, "angle"), 0.0));
        w.bd(num(field(g, "shift"), 0.0));
        w.bl(if truthy(field(g, "single")) { 1.0 } else { 0.0 });
        w.bd(num(field(g, "tint"), 0.0));
        let cols = arr(field(g, "colors"));
        w.bl(usize_to_f64(cols.len()));
        for c in cols {
            w.bd(num(field(Some(c), "value"), 0.0));
            w.bs(0.0);
            let rgb = crate::util::u32_from_i32(to_i32(num(field(Some(c), "rgb"), 0.0)));
            w.bl(f64::from(crate::util::i32_from_u32(0xC200_0000 | (rgb & 0xFF_FFFF))));
            w.rc8(0);
        }
        w.tv(&if truthy(field(g, "name")) { js_string(field(g, "name")) } else { String::new() });
    }
    w.bd(num(r.g("elevation"), 0.0));
    ext3(w, r.g("extrusion"));
    let solid = truthy(r.g("solid"));
    w.tv(&if truthy(r.g("pattern")) {
        js_string(r.g("pattern"))
    } else if solid {
        "SOLID".into()
    } else {
        "ANSI31".into()
    });
    w.b(solid);
    w.b(false);
    let loops = arr(r.g("loops"));
    w.bl(usize_to_f64(loops.len()));
    let mut pixel = false;
    for l in loops {
        let l = Some(l);
        let poly = truthy(field(l, "polyline"));
        let flag = (int32(field(l, "flags")) & !2 & !0x200) | if poly { 2 } else { 0 };
        if flag & 4 != 0 {
            pixel = true;
        }
        w.bl(f64::from(flag));
        if !poly {
            let edges = arr(field(l, "edges"));
            w.bl(usize_to_f64(edges.len()));
            for e in edges {
                let e = Some(e);
                let ty = field(e, "type");
                let t: f64 = match ty {
                    Some(V::Str(s)) if s == "line" => 1.0,
                    Some(V::Str(s)) if s == "arc" => 2.0,
                    Some(V::Str(s)) if s == "ellipse" => 3.0,
                    Some(V::Str(s)) if s == "spline" => 4.0,
                    Some(V::Num(n)) => *n,
                    other => to_number(other),
                };
                w.rc(t);
                let xy = |k: &str| (num(field(field(e, k), "x"), 0.0), num(field(field(e, k), "y"), 0.0));
                if t == 1.0 {
                    let s = xy("start");
                    let en = xy("end");
                    w.rd2(s.0, s.1);
                    w.rd2(en.0, en.1);
                } else if t == 2.0 {
                    let c = xy("center");
                    w.rd2(c.0, c.1);
                    w.bd(num(field(e, "radius"), 0.0));
                    w.bd(num(field(e, "startAngle"), 0.0));
                    w.bd(num(field(e, "endAngle"), 0.0));
                    w.b(!matches!(field(e, "ccw"), Some(V::Bool(false))));
                } else if t == 3.0 {
                    let c = xy("center");
                    let m = xy("majorAxis");
                    w.rd2(c.0, c.1);
                    w.rd2(m.0, m.1);
                    w.bd(num(field(e, "ratio"), 1.0));
                    w.bd(num(field(e, "startAngle"), 0.0));
                    w.bd(num(field(e, "endAngle"), 0.0));
                    w.b(!matches!(field(e, "ccw"), Some(V::Bool(false))));
                } else if t == 4.0 {
                    let ctl = arr(field(e, "control"));
                    let kn = arr(field(e, "knots"));
                    let rational = ctl.iter().any(|p| {
                        let wv = field(Some(p), "w");
                        wv.is_some() && !matches!(wv, Some(V::Num(n)) if *n == 1.0)
                    });
                    w.bl(ornum(field(e, "degree"), 3.0));
                    w.b(rational);
                    w.b(truthy(field(e, "periodic")));
                    w.bl(usize_to_f64(kn.len()));
                    w.bl(usize_to_f64(ctl.len()));
                    for k in kn {
                        w.bd(num(Some(k), 0.0));
                    }
                    for p in ctl {
                        w.rd2(num(field(Some(p), "x"), 0.0), num(field(Some(p), "y"), 0.0));
                        if rational {
                            w.bd(num(field(Some(p), "w"), 1.0));
                        }
                    }
                    if w.r2010 {
                        let fit = arr(field(e, "fit"));
                        w.bl(usize_to_f64(fit.len()));
                        if !fit.is_empty() {
                            for p in fit {
                                w.rd2(num(field(Some(p), "x"), 0.0), num(field(Some(p), "y"), 0.0));
                            }
                            let st = xy("startTangent");
                            let et = xy("endTangent");
                            w.rd2(st.0, st.1);
                            w.rd2(et.0, et.1);
                        }
                    }
                }
            }
        } else {
            let pl = field(l, "polyline");
            let pv = arr(field(pl, "vertices"));
            let has_b = pv.iter().any(|p| num(field(Some(p), "bulge"), 0.0) != 0.0);
            w.b(has_b);
            w.b(!matches!(field(pl, "closed"), Some(V::Bool(false))));
            w.bl(usize_to_f64(pv.len()));
            for p in pv {
                w.rd2(num(field(Some(p), "x"), 0.0), num(field(Some(p), "y"), 0.0));
                if has_b {
                    w.bd(num(field(Some(p), "bulge"), 0.0));
                }
            }
        }
        w.bl(0.0); // no boundary objects
    }
    w.bs(f64::from(int32(r.g("style"))));
    w.bs(nnum(r.g("patternType"), 1.0));
    if !solid {
        w.bd(num(r.g("angle"), 0.0));
        w.bd(num(r.g("scale"), 1.0));
        w.b(truthy(r.g("double")));
        let lines = arr(r.g("lines"));
        w.bs(usize_to_f64(lines.len()));
        for ln in lines {
            let ln = Some(ln);
            w.bd(num(field(ln, "angle"), 0.0));
            w.bd2(num(field(field(ln, "base"), "x"), 0.0), num(field(field(ln, "base"), "y"), 0.0));
            w.bd2(num(field(field(ln, "offset"), "x"), 0.0), num(field(field(ln, "offset"), "y"), 0.0));
            let dashes = arr(field(ln, "dashes"));
            w.bs(usize_to_f64(dashes.len()));
            for d in dashes {
                w.bd(num(Some(d), 0.0));
            }
        }
    }
    if pixel {
        w.bd(num(r.g("pixelSize"), 1.0));
    }
    let seeds = arr(r.g("seeds"));
    w.bl(usize_to_f64(seeds.len()));
    for s in seeds {
        w.rd2(num(field(Some(s), "x"), 0.0), num(field(Some(s), "y"), 0.0));
    }
}

fn dim_common(w: &mut BitWriter, r: &Rec) {
    entity_common(w, r);
    if w.r2010 {
        w.rc8(0);
    }
    ext3(w, r.g("extrusion"));
    let tm = r.g("textMid");
    w.rd2(num(field(tm, "x"), 0.0), num(field(tm, "y"), 0.0));
    let elev = nn(r.g("elevation")).or_else(|| field(tm, "z"));
    w.bd(num(elev, 0.0));
    match nn(r.g("flags1")) {
        Some(v) => w.rc(to_number(Some(v))),
        None => w.rc(if truthy(r.g("textMidUser")) { 0.0 } else { 1.0 }),
    }
    w.tv(&str_or(r.g("text"), ""));
    w.bd(num(r.g("textRotation"), 0.0));
    w.bd(num(r.g("horizDir"), 0.0));
    match r.g("insScale").filter(|v| v.truthy()) {
        Some(s) => w.p3(Some(s)),
        None => w.p3n(1.0, 1.0, 1.0),
    }
    w.bd(num(r.g("insRotation"), 0.0));
    w.bs(ornum(r.g("attach"), 5.0));
    w.bs(ornum(r.g("lineSpacingStyle"), 1.0));
    w.bd(num(r.g("lineSpacing"), 1.0));
    w.bd(num(r.g("measurement"), 0.0));
    if w.r2007 {
        w.b(false);
        w.b(truthy(r.g("flipArrow1")));
        w.b(truthy(r.g("flipArrow2")));
    }
    let ins = r.g("insert");
    w.rd2(num(field(ins, "x"), 0.0), num(field(ins, "y"), 0.0));
}
fn dim_handles(w: &mut BitWriter, r: &Rec) {
    w.hv(HARD_PTR, r.g("dimstyle"));
    w.hv(HARD_PTR, r.g("block"));
}

fn image_entity(w: &mut BitWriter, r: &Rec) {
    entity_common(w, r);
    w.bl(f64::from(int32(r.g("classVersion"))));
    w.p3(r.g("insert"));
    w.p3(r.g("u"));
    w.p3(r.g("v"));
    w.rd2(num(field(r.g("size"), "x"), 1.0), num(field(r.g("size"), "y"), 1.0));
    w.bs(nnum(r.g("displayFlags"), 7.0));
    w.b(truthy(r.g("clipping")));
    w.rc(nnum(r.g("brightness"), 50.0));
    w.rc(nnum(r.g("contrast"), 50.0));
    w.rc(f64::from(int32(r.g("fade"))));
    if w.r2010 {
        w.b(truthy(r.g("clipMode")));
    }
    let clip = arr(r.g("clip"));
    let ty = if matches!(r.g("clipType"), Some(V::Num(n)) if *n == 1.0) && clip.len() >= 2 { 1 } else { 2 };
    w.bs(f64::from(ty));
    let xy = |p: Option<&V>| (num(field(p, "x"), 0.0), num(field(p, "y"), 0.0));
    if ty == 1 {
        let a = xy(clip.first());
        let b = xy(clip.get(1));
        w.rd2(a.0, a.1);
        w.rd2(b.0, b.1);
    } else {
        w.bl(usize_to_f64(clip.len()));
        for p in clip {
            let a = xy(Some(p));
            w.rd2(a.0, a.1);
        }
    }
    w.hv(HARD_PTR, r.g("def"));
    w.hv(HARD_OWN, r.g("defReactor"));
}

fn viewport(w: &mut BitWriter, r: &Rec) {
    entity_common(w, r);
    w.p3(r.g("center"));
    w.bd(num(r.g("width"), 1.0));
    w.bd(num(r.g("height"), 1.0));
    w.p3(r.g("viewTarget"));
    let vd = r.g("viewDirection").filter(|v| v.truthy()).cloned().unwrap_or_else(|| p3v(0.0, 0.0, 1.0));
    ext3(w, Some(&vd));
    w.bd(num(r.g("twist"), 0.0));
    w.bd(num(r.g("viewHeight"), num(r.g("height"), 1.0)));
    w.bd(num(r.g("lens"), 50.0));
    w.bd(0.0);
    w.bd(0.0);
    w.bd(0.0);
    w.rd2(num(field(r.g("viewCenter"), "x"), 0.0), num(field(r.g("viewCenter"), "y"), 0.0));
    w.rd2(0.0, 0.0);
    w.rd2(10.0, 10.0);
    w.rd2(10.0, 10.0);
    w.bs(1000.0);
    if w.r2007 {
        w.bs(5.0);
    }
    let frozen = arr(r.g("frozenLayers"));
    w.bl(usize_to_f64(frozen.len()));
    w.bl(nnum(r.g("status"), f64::from(0x8060)));
    w.tv("");
    w.rc8(0);
    w.b(false);
    w.b(false);
    w.bd3(0.0, 0.0, 0.0);
    w.bd3(1.0, 0.0, 0.0);
    w.bd3(0.0, 1.0, 0.0);
    w.bd(0.0);
    w.bs(0.0);
    if w.r2004 {
        w.bs(0.0);
    }
    if w.r2007 {
        w.b(true);
        w.rc8(1);
        w.bd(0.0);
        w.bd(0.0);
        w.cmc(Some(&V::Obj(obj! { "index" => 250 })));
    }
    let code = if w.r2004 { SOFT_PTR } else { HARD_PTR };
    for h in frozen {
        w.hv(code, Some(h));
    }
    w.h(SOFT_PTR, 0.0);
    if !w.r2004 {
        w.h(HARD_PTR, 0.0);
    }
    w.h(HARD_PTR, 0.0);
    w.h(HARD_PTR, 0.0);
    if w.r2007 {
        w.h(SOFT_PTR, 0.0);
        w.h(HARD_PTR, 0.0);
        w.h(SOFT_PTR, 0.0);
        w.h(HARD_OWN, 0.0);
    }
}

/// The ENC table: true when `t` has an encoder (and writes it).
pub fn enc_std(w: &mut BitWriter, r: &mut Rec, cx: &mut Cx) -> bool {
    let t = r.t().to_string();
    match t.as_str() {
        "BLOCK_CONTROL" | "LAYER_CONTROL" | "STYLE_CONTROL" | "LTYPE_CONTROL" | "VIEW_CONTROL" | "UCS_CONTROL" | "VPORT_CONTROL"
        | "APPID_CONTROL" | "DIMSTYLE_CONTROL" | "VPENT_CONTROL" => control(w, r),
        "BLOCK_HEADER" => block_header(w, r),
        "LAYER" => layer(w, r),
        "STYLE" => style(w, r),
        "LTYPE" => ltype(w, r),
        "UCS" => ucs(w, r),
        "VPORT" => vport(w, r),
        "VIEW" => view(w, r),
        "APPID" => appid(w, r),
        "DIMSTYLE" => dimstyle(w, r),
        "DICTIONARY" => dictionary(w, r),
        "DICTIONARYWDFLT" => {
            dictionary(w, r);
            w.hv(HARD_PTR, r.g("defaultEntry"));
        }
        "PLACEHOLDER" => object_common(w, r),
        "XRECORD" => xrecord(w, r),
        "MLINESTYLE" => mlinestyle(w, r),
        "LAYOUT" => layout(w, r),
        "BLOCK" => {
            entity_common(w, r);
            w.tv(&str_or(r.g("name"), ""));
        }
        "ENDBLK" | "SEQEND" => entity_common(w, r),
        "LINE" => line(w, r),
        "POINT" => {
            entity_common(w, r);
            w.p3(r.g("position"));
            w.bt(num(r.g("thickness"), 0.0));
            bex(w, r.g("extrusion"));
            w.bd(num(r.g("xAxisAngle"), 0.0));
        }
        "CIRCLE" => {
            entity_common(w, r);
            w.p3(r.g("center"));
            w.bd(num(r.g("radius"), 0.0));
            w.bt(num(r.g("thickness"), 0.0));
            bex(w, r.g("extrusion"));
        }
        "ARC" => {
            entity_common(w, r);
            w.p3(r.g("center"));
            w.bd(num(r.g("radius"), 0.0));
            w.bt(num(r.g("thickness"), 0.0));
            bex(w, r.g("extrusion"));
            w.bd(rad(r.g("startAngle")));
            w.bd(rad(r.g("endAngle")));
        }
        "ELLIPSE" => {
            entity_common(w, r);
            w.p3(r.g("center"));
            w.p3(r.g("majorAxis"));
            ext3(w, r.g("extrusion"));
            w.bd(num(r.g("ratio"), 1.0));
            w.bd(num(r.g("startParam"), 0.0));
            w.bd(num(r.g("endParam"), 2.0 * std::f64::consts::PI));
        }
        "RAY" | "XLINE" => {
            entity_common(w, r);
            w.p3(r.g("start"));
            w.p3(r.g("direction"));
        }
        "SOLID" | "TRACE" => corners(w, r),
        "3DFACE" => face3d(w, r),
        "TEXT" => {
            entity_common(w, r);
            text_data(w, r);
            w.hv(HARD_PTR, r.g("style"));
        }
        "ATTRIB" => {
            entity_common(w, r);
            text_data(w, r);
            attrib_tail(w, r);
            w.hv(HARD_PTR, r.g("style"));
        }
        "ATTDEF" => {
            entity_common(w, r);
            text_data(w, r);
            attrib_tail(w, r);
            if w.r2010 {
                w.rc8(0);
            }
            w.tv(&str_or(r.g("prompt"), ""));
            w.hv(HARD_PTR, r.g("style"));
        }
        "MTEXT" => mtext(w, r),
        "INSERT" => insert(w, r, false),
        "MINSERT" => insert(w, r, true),
        "POLYLINE_2D" => {
            entity_common(w, r);
            w.bs(f64::from(int32(r.g("flags"))));
            w.bs(f64::from(int32(r.g("curveType"))));
            w.bd(num(r.g("startWidth"), 0.0));
            w.bd(num(r.g("endWidth"), 0.0));
            w.bt(num(r.g("thickness"), 0.0));
            w.bd(num(r.g("elevation"), 0.0));
            bex(w, r.g("extrusion"));
            if w.r2004 {
                w.bl(n_verts(r));
            }
            poly_owned(w, r);
        }
        "POLYLINE_3D" => {
            entity_common(w, r);
            w.rc(f64::from(int32(r.g("splineFlags"))));
            w.rc8(u8::from(truthy(r.g("closed"))));
            if w.r2004 {
                w.bl(n_verts(r));
            }
            poly_owned(w, r);
        }
        "POLYLINE_PFACE" => {
            entity_common(w, r);
            w.bs(f64::from(int32(r.g("numVerts"))));
            w.bs(f64::from(int32(r.g("numFaces"))));
            if w.r2004 {
                w.bl(n_verts(r));
            }
            poly_owned(w, r);
        }
        "POLYLINE_MESH" => {
            entity_common(w, r);
            for k in ["flags", "curveType", "m", "n", "mDensity", "nDensity"] {
                w.bs(f64::from(int32(r.g(k))));
            }
            if w.r2004 {
                w.bl(n_verts(r));
            }
            poly_owned(w, r);
        }
        "VERTEX_2D" => {
            entity_common(w, r);
            w.rc(f64::from(int32(r.g("flags"))));
            let p = r.g("point");
            w.p3n(or0v(field(p, "x")), or0v(field(p, "y")), 0.0);
            let sw = num(r.g("startWidth"), 0.0);
            let ew = num(r.g("endWidth"), sw);
            if sw != 0.0 && sw == ew {
                w.bd(-sw.abs());
            } else {
                w.bd(sw);
                w.bd(ew);
            }
            w.bd(num(r.g("bulge"), 0.0));
            if w.r2010 {
                w.bl(f64::from(int32(r.g("id"))));
            }
            w.bd(num(r.g("tangent"), 0.0));
        }
        "VERTEX_3D" | "VERTEX_MESH" | "VERTEX_PFACE" => {
            entity_common(w, r);
            w.rc(f64::from(int32(r.g("flags"))));
            w.p3(r.g("point"));
        }
        "VERTEX_PFACE_FACE" => {
            entity_common(w, r);
            let idx = arr(r.g("indices"));
            for i in 0..4usize {
                w.bs(f64::from(int32(idx.get(i))));
            }
        }
        "LWPOLYLINE" => lwpolyline(w, r),
        "SPLINE" => spline(w, r),
        "HATCH" => hatch(w, r),
        "DIMENSION_ORDINATE" => {
            dim_common(w, r);
            w.p3(r.g("defPoint"));
            w.p3(r.g("xline1"));
            w.p3(r.g("xline2"));
            w.rc8(u8::from(truthy(r.g("ordinateX"))));
            dim_handles(w, r);
        }
        "DIMENSION_LINEAR" => {
            dim_common(w, r);
            w.p3(r.g("xline1"));
            w.p3(r.g("xline2"));
            w.p3(r.g("defPoint"));
            w.bd(num(r.g("obliqueAngle"), 0.0));
            w.bd(num(r.g("angle"), 0.0));
            dim_handles(w, r);
        }
        "DIMENSION_ALIGNED" => {
            dim_common(w, r);
            w.p3(r.g("xline1"));
            w.p3(r.g("xline2"));
            w.p3(r.g("defPoint"));
            w.bd(num(r.g("obliqueAngle"), 0.0));
            dim_handles(w, r);
        }
        "DIMENSION_ANG3PT" => {
            dim_common(w, r);
            w.p3(r.g("defPoint"));
            w.p3(r.g("xline1"));
            w.p3(r.g("xline2"));
            w.p3(r.g("center"));
            dim_handles(w, r);
        }
        "DIMENSION_ANG2LN" => {
            dim_common(w, r);
            let ap = r.g("arcPoint");
            w.rd2(num(field(ap, "x"), 0.0), num(field(ap, "y"), 0.0));
            w.p3(r.g("xline1"));
            w.p3(r.g("xline2"));
            w.p3(nn(r.g("xline1End")).or_else(|| r.g("center")));
            w.p3(r.g("defPoint"));
            dim_handles(w, r);
        }
        "DIMENSION_RADIUS" => {
            dim_common(w, r);
            w.p3(r.g("defPoint"));
            w.p3(r.g("chordPoint"));
            w.bd(num(r.g("leaderLen"), 0.0));
            dim_handles(w, r);
        }
        "DIMENSION_DIAMETER" => {
            dim_common(w, r);
            w.p3(r.g("chordPoint"));
            w.p3(r.g("defPoint"));
            w.bd(num(r.g("leaderLen"), 0.0));
            dim_handles(w, r);
        }
        "ARC_DIMENSION" => {
            dim_common(w, r);
            w.p3(r.g("defPoint"));
            w.p3(r.g("xline1"));
            w.p3(r.g("xline2"));
            w.p3(r.g("center"));
            w.b(truthy(r.g("isPartial")));
            w.bd(num(r.g("arcStart"), 0.0));
            w.bd(num(r.g("arcEnd"), 0.0));
            w.b(truthy(r.g("hasLeader")));
            w.p3(r.g("leader1"));
            w.p3(r.g("leader2"));
            dim_handles(w, r);
        }
        "LARGE_RADIAL_DIMENSION" => {
            dim_common(w, r);
            w.p3(r.g("defPoint"));
            w.p3(r.g("chordPoint"));
            w.bd(0.0);
            w.p3(r.g("overrideCenter"));
            w.p3(r.g("jogPoint"));
            dim_handles(w, r);
        }
        "IMAGE" | "WIPEOUT" => image_entity(w, r),
        "IMAGEDEF" => {
            object_common(w, r);
            w.bl(f64::from(int32(r.g("classVersion"))));
            w.rd2(num(field(r.g("size"), "x"), 1.0), num(field(r.g("size"), "y"), 1.0));
            w.tv(&if truthy(r.g("fileName")) { js_string(r.g("fileName")) } else { String::new() });
            w.b(!matches!(r.g("loaded"), Some(V::Bool(false))));
            w.rc(f64::from(int32(r.g("resUnits"))));
            w.rd2(num(field(r.g("pixelSize"), "x"), 1.0), num(field(r.g("pixelSize"), "y"), 1.0));
        }
        "IMAGEDEF_REACTOR" => {
            object_common(w, r);
            w.bl(nnum(r.g("classVersion"), 2.0));
        }
        "RASTERVARIABLES" => {
            object_common(w, r);
            w.bl(0.0);
            w.bs(nnum(r.g("frame"), 1.0));
            w.bs(nnum(r.g("quality"), 1.0));
            w.bs(f64::from(int32(r.g("units"))));
        }
        // WIPEOUTVARIABLES: not in the spec; AutoCAD's own hold one BS, the display frame.
        "WIPEOUTVARIABLES" => {
            object_common(w, r);
            w.bs(nnum(r.g("frame"), 1.0));
        }
        "LEADER" => {
            entity_common(w, r);
            w.b(false);
            w.bs(nnum(r.g("annotType"), 3.0));
            w.bs(f64::from(int32(r.g("pathType"))));
            let v = arr(r.g("vertices"));
            w.bl(usize_to_f64(v.len()));
            for p in v {
                w.p3(Some(p));
            }
            match r.g("origin").filter(|x| x.truthy()) {
                Some(o) => w.p3(Some(o)),
                None => w.p3(v.first()),
            }
            ext3(w, r.g("extrusion"));
            match r.g("xDirection").filter(|x| x.truthy()) {
                Some(x) => w.p3(Some(x)),
                None => w.p3n(1.0, 0.0, 0.0),
            }
            w.p3(r.g("blockOffset"));
            w.p3(r.g("endPointProj"));
            // Spec gap (builder R's reader, ODA-saved R2007/R2010 leaders): from R2010 there is no box height/width.
            if !w.r2010 {
                w.bd(num(r.g("boxHeight"), 0.0));
                w.bd(num(r.g("boxWidth"), 0.0));
            }
            w.b(truthy(r.g("hookLineOnX")));
            w.b(!matches!(r.g("arrowhead"), Some(V::Bool(false))));
            w.bs(0.0);
            w.b(false);
            w.b(false);
            w.hv(HARD_PTR, r.g("annotation"));
            w.hv(HARD_PTR, r.g("dimstyle"));
        }
        "TOLERANCE" => {
            entity_common(w, r);
            w.p3(r.g("insert"));
            match r.g("xDirection").filter(|x| x.truthy()) {
                Some(x) => w.p3(Some(x)),
                None => w.p3n(1.0, 0.0, 0.0),
            }
            ext3(w, r.g("extrusion"));
            w.tv(&str_or(r.g("text"), ""));
            w.hv(HARD_PTR, r.g("dimstyle"));
        }
        "MLINE" => {
            entity_common(w, r);
            w.bd(num(r.g("scale"), 1.0));
            w.rc(f64::from(int32(r.g("justification"))));
            let vs = arr(r.g("vertices"));
            match r.g("base").filter(|x| x.truthy()) {
                Some(b) => w.p3(Some(b)),
                None => w.p3(vs.first().and_then(|v| field(Some(v), "point"))),
            }
            ext3(w, r.g("extrusion"));
            w.bs(if truthy(r.g("closed")) { 3.0 } else { 1.0 });
            let nl = to_number(r.g("lines"));
            w.rc(nl);
            w.bs(usize_to_f64(vs.len()));
            let nlu = crate::util::to_usize(nl).min(256);
            for v in vs {
                let v = Some(v);
                w.p3(field(v, "point"));
                w.p3(field(v, "direction"));
                w.p3(field(v, "miter"));
                let lines = arr(field(v, "lines"));
                for k in 0..nlu {
                    let l = lines.get(k).filter(|x| x.truthy());
                    let segs = arr(field(l, "segments"));
                    w.bs(usize_to_f64(segs.len()));
                    for x in segs {
                        w.bd(num(Some(x), 0.0));
                    }
                    let fill = arr(field(l, "fill"));
                    w.bs(usize_to_f64(fill.len()));
                    for x in fill {
                        w.bd(num(Some(x), 0.0));
                    }
                }
            }
            w.hv(HARD_PTR, r.g("style"));
        }
        "SHAPE" => {
            entity_common(w, r);
            w.p3(r.g("insert"));
            w.bd(num(r.g("scale"), 1.0));
            w.bd(num(r.g("rotation"), 0.0));
            w.bd(num(r.g("widthFactor"), 1.0));
            w.bd(num(r.g("oblique"), 0.0));
            w.bd(num(r.g("thickness"), 0.0));
            w.bs(f64::from(int32(r.g("shapeNumber"))));
            ext3(w, r.g("extrusion"));
            w.hv(HARD_PTR, r.g("style"));
        }
        "VIEWPORT" => viewport(w, r),
        "FIELD" | "TABLESTYLE" | "ACAD_TABLE" => return crate::w_table::enc_table(w, r, cx),
        _ => return false,
    }
    true
}
/// `p.x` as P3 reads it when only x/y are given (`{x: r.point.x, y: r.point.y, z: 0}` then `p?.x || 0`)
fn or0v(v: Option<&V>) -> f64 {
    crate::w_bits::or0(v)
}

/// Does the ENC table have an encoder for `t`?
pub fn has_enc(t: &str) -> bool {
    matches!(
        t,
        "BLOCK_CONTROL"
            | "LAYER_CONTROL"
            | "STYLE_CONTROL"
            | "LTYPE_CONTROL"
            | "VIEW_CONTROL"
            | "UCS_CONTROL"
            | "VPORT_CONTROL"
            | "APPID_CONTROL"
            | "DIMSTYLE_CONTROL"
            | "VPENT_CONTROL"
            | "BLOCK_HEADER"
            | "LAYER"
            | "STYLE"
            | "LTYPE"
            | "UCS"
            | "VPORT"
            | "VIEW"
            | "APPID"
            | "DIMSTYLE"
            | "DICTIONARY"
            | "DICTIONARYWDFLT"
            | "PLACEHOLDER"
            | "XRECORD"
            | "MLINESTYLE"
            | "LAYOUT"
            | "BLOCK"
            | "ENDBLK"
            | "SEQEND"
            | "LINE"
            | "POINT"
            | "CIRCLE"
            | "ARC"
            | "ELLIPSE"
            | "RAY"
            | "XLINE"
            | "SOLID"
            | "TRACE"
            | "3DFACE"
            | "TEXT"
            | "ATTRIB"
            | "ATTDEF"
            | "MTEXT"
            | "INSERT"
            | "MINSERT"
            | "POLYLINE_2D"
            | "POLYLINE_3D"
            | "POLYLINE_PFACE"
            | "POLYLINE_MESH"
            | "VERTEX_2D"
            | "VERTEX_3D"
            | "VERTEX_MESH"
            | "VERTEX_PFACE"
            | "VERTEX_PFACE_FACE"
            | "LWPOLYLINE"
            | "SPLINE"
            | "HATCH"
            | "DIMENSION_ORDINATE"
            | "DIMENSION_LINEAR"
            | "DIMENSION_ALIGNED"
            | "DIMENSION_ANG3PT"
            | "DIMENSION_ANG2LN"
            | "DIMENSION_RADIUS"
            | "DIMENSION_DIAMETER"
            | "ARC_DIMENSION"
            | "LARGE_RADIAL_DIMENSION"
            | "IMAGE"
            | "WIPEOUT"
            | "IMAGEDEF"
            | "IMAGEDEF_REACTOR"
            | "RASTERVARIABLES"
            | "WIPEOUTVARIABLES"
            | "LEADER"
            | "TOLERANCE"
            | "MLINE"
            | "SHAPE"
            | "VIEWPORT"
            | "FIELD"
            | "TABLESTYLE"
            | "ACAD_TABLE"
    )
}

/// Encode one record. None for an unknown type.
pub fn encode_object(r: &mut Rec, cx: &mut Cx) -> Option<Vec<u8>> {
    match std::mem::take(&mut r.enc) {
        Enc::Std => {}
        other => {
            let out = encode_custom(&other, r, cx);
            r.enc = other;
            return out;
        }
    }
    let t = r.t().to_string();
    if !has_enc(&t) {
        return None;
    }
    let ty = if cx.classes.has(&t) { cx.classes.class_num(&t) } else { fixed_type(&t)? };
    let mut w = BitWriter::obj_writer(cx.version, false);
    begin(&mut w, ty, r);
    enc_std(&mut w, r, cx);
    Some(finish(&mut w))
}
fn encode_custom(e: &Enc, r: &mut Rec, cx: &mut Cx) -> Option<Vec<u8>> {
    match e {
        Enc::Std => None,
        Enc::Raw(x) => crate::w_passthrough::encode_raw(x, r, cx),
        Enc::Mleader(x) => Some(crate::w_mleader::encode_mleader(x, r, cx)),
        Enc::MleaderStyle(x) => Some(crate::w_mleader::encode_mleader_style(x, r, cx)),
        Enc::XrecordCopy(x) => Some(crate::w_nongraphic::encode_xrecord(x, r, cx)),
        Enc::FieldCopy(x) => crate::w_nongraphic::encode_field_copy(x, r, cx),
        Enc::Light => Some(crate::w_solid::encode_light(r, cx)),
        Enc::Acis(x) => Some(crate::w_solid::encode_acis_entity(x, r, cx)),
        Enc::Surface(x) => Some(crate::w_surface::encode_surface(x, r, cx)),
    }
}

// ---------------------------------------------------------------- classes (ch. 10, 5.8)
pub fn encode_classes(classes: &Classes, version: &str, dwg_version: u32, maint: u32) -> Vec<u8> {
    let mut w = BitWriter::obj_writer(version, false);
    let max_num = classes.list.iter().fold(499.0f64, |m, c| m.max(c.num));
    if w.r2007 {
        w.size_at = w.bit;
        w.rl_u(0);
    }
    if w.r2004 {
        w.bs(max_num);
        w.rc8(0);
        w.rc8(0);
        w.b(true);
    }
    for c in &classes.list {
        let p = c.props.clone().unwrap_or_default();
        w.bs(c.num);
        w.bs(f64::from(p.proxy));
        w.tv(&str_or(p.app.as_ref(), ""));
        w.tv(&str_or(p.cpp.as_ref(), ""));
        w.tv(&str_or(p.dxf.as_ref(), ""));
        w.b(false);
        w.bs(if p.entity { 498.0 } else { 499.0 });
        if w.r2004 {
            w.bl(f64::from(c.count));
            w.bs(f64::from(dwg_version));
            w.bs(f64::from(maint));
            w.bl(0.0);
            w.bl(0.0);
        }
    }
    let endbit = w.end_data();
    if w.r2007 {
        let at = w.size_at;
        w.patch_rl(at, crate::util::u32_of_usize(endbit));
        w.append_hnd();
    }
    w.bytes()
}

// ---------------------------------------------------------------- header variables (ch. 9)
/// julian(d): { day, ms } of a time in ms since 1970.
pub fn julian(t: f64) -> crate::w_container::Jd {
    let days = (t / 86_400_000.0).floor();
    crate::w_container::Jd { day: days + 2_440_588.0, ms: t - days * 86_400_000.0 }
}
/// Writer-side values for header variables the table gives no default for.
fn header_default(name: &str) -> Option<V> {
    let pt2 = |x: f64, y: f64| crate::value::p2(x, y);
    Some(match name {
        "UNKNOWN_9" => V::Num(24.0),
        "MENU" => V::from("acad"),
        "DIMASO" | "DIMSHO" | "REGENMODE" | "FILLMODE" | "PSLTSCALE" | "USRTIMER" | "WORLDVIEW" | "TILEMODE" | "VISRETAIN"
        | "PROXYGRAPHICS" | "ATTMODE" | "LTSCALE" | "CMLSCALE" | "CELTSCALE" | "TSTACKALIGN" | "HIDETEXT" | "SHOWHIST" | "LOFTNORMALS"
        | "LIGHTGLYPHDISPLAY" | "TILEMODELIGHTSYNCH" | "PSTYLEMODE" => V::Num(1.0),
        "TREEDEPTH" => V::Num(3020.0),
        "LUNITS" | "DIMASSOC" | "STEPSPERSEC" | "3DDWFPREC" | "DWFFRAME" => V::Num(2.0),
        "LUPREC" | "ISOLINES" => V::Num(4.0),
        "SPLINESEGS" => V::Num(8.0),
        "SURFU" | "SURFV" | "SURFTYPE" | "SURFTAB1" | "SURFTAB2" | "SPLINETYPE" | "STEPSIZE" => V::Num(6.0),
        "SHADEDGE" => V::Num(3.0),
        "SHADEDIF" | "TSTACKSIZE" => V::Num(70.0),
        "MAXACTVP" => V::Num(64.0),
        "TEXTQLTY" | "LENSLENGTH" => V::Num(50.0),
        "TEXTSIZE" => V::Num(0.2),
        "TRACEWID" => V::Num(0.05),
        "SKETCHINC" => V::Num(0.1),
        "FACETRES" => V::Num(0.5),
        "PUCSXDIR" | "UCSXDIR" => p3v(1.0, 0.0, 0.0),
        "PUCSYDIR" | "UCSYDIR" => p3v(0.0, 1.0, 0.0),
        "PLIMMIN" | "LIMMIN" => pt2(0.0, 0.0),
        "PLIMMAX" | "LIMMAX" => pt2(12.0, 9.0),
        "SORTENTS" => V::Num(127.0),
        "OBSCUREDCOLOR" | "INTERSECTIONCOLOR" => V::Num(257.0),
        "PSOLWIDTH" => V::Num(0.25),
        "PSOLHEIGHT" => V::Num(4.0),
        "LOFTANG1" | "LOFTANG2" => V::Num(std::f64::consts::PI / 2.0),
        "LOFTPARAM" => V::Num(7.0),
        "LATITUDE" => V::Num(37.795),
        "LONGITUDE" => V::Num(-122.394),
        "TIMEZONE" => V::Num(-8000.0),
        "INTERFERECOLOR" => V::Obj(obj! { "index" => 1 }),
        "CECOLOR" => V::Obj(obj! { "index" => 256 }),
        "DIMLWD" | "DIMLWE" => V::Num(-2.0),
        _ => return None,
    })
}
/// Header handle variables → the writer's handle names.
fn header_handle(name: &str) -> Option<&'static str> {
    Some(match name {
        "CLAYER" => "clayer",
        "TEXTSTYLE" | "DIMTXSTY" => "textstyle",
        "CELTYPE" => "celtype",
        "CMATERIAL" => "cmaterial",
        "DIMSTYLE" => "dimstyleCur",
        "CMLSTYLE" => "cmlstyle",
        "BLOCK_CONTROL_OBJECT" => "blockCtl",
        "LAYER_CONTROL_OBJECT" => "layerCtl",
        "STYLE_CONTROL_OBJECT" => "styleCtl",
        "LINETYPE_CONTROL_OBJECT" => "ltypeCtl",
        "VIEW_CONTROL_OBJECT" => "viewCtl",
        "UCS_CONTROL_OBJECT" => "ucsCtl",
        "VPORT_CONTROL_OBJECT" => "vportCtl",
        "APPID_CONTROL_OBJECT" => "appidCtl",
        "DIMSTYLE_CONTROL_OBJECT" => "dimstyleCtl",
        "VPORT_ENTITY_HEADER_CONTROL_OBJECT" => "vpentCtl",
        "DICTIONARY_ACAD_GROUP" => "groupDict",
        "DICTIONARY_ACAD_MLINESTYLE" => "mlsDict",
        "DICTIONARY_NAMED_OBJECTS" => "nod",
        "DICTIONARY_LAYOUTS" => "layoutDict",
        "DICTIONARY_PLOTSETTINGS" => "plotSettingsDict",
        "DICTIONARY_PLOTSTYLES" => "plotStyleDict",
        "DICTIONARY_MATERIALS" => "materialsDict",
        "DICTIONARY_COLORS" => "colorsDict",
        "DICTIONARY_VISUALSTYLE" => "visualStyleDict",
        "BLOCK_RECORD_PAPER_SPACE" => "psBlockHdr",
        "BLOCK_RECORD_MODEL_SPACE" => "msBlockHdr",
        "LTYPE_BYLAYER" => "ltByLayer",
        "LTYPE_BYBLOCK" => "ltByBlock",
        "LTYPE_CONTINUOUS" => "ltContinuous",
        _ => return None,
    })
}
fn hard_owned(name: &str) -> bool {
    matches!(
        name,
        "BLOCK_CONTROL_OBJECT"
            | "LAYER_CONTROL_OBJECT"
            | "STYLE_CONTROL_OBJECT"
            | "LINETYPE_CONTROL_OBJECT"
            | "VIEW_CONTROL_OBJECT"
            | "UCS_CONTROL_OBJECT"
            | "VPORT_CONTROL_OBJECT"
            | "APPID_CONTROL_OBJECT"
            | "DIMSTYLE_CONTROL_OBJECT"
            | "VPORT_ENTITY_HEADER_CONTROL_OBJECT"
            | "DICTIONARY_NAMED_OBJECTS"
    )
}

/// Model-space extents: (min, max) as (x, y, z).
pub type Bounds = ((f64, f64, f64), (f64, f64, f64));

/// What encodeHeader takes from write.js's ctx.
pub struct HeaderCtx {
    pub version: &'static str,
    pub metric: bool,
    pub created: f64,
    pub handseed: f64,
    pub extents: Option<Bounds>,
    pub guid1: String,
    pub guid2: String,
}

/// The AcDb:Header data, written by walking the shared table (header_vars.rs, ch. 9) for the version.
/// `hh` = the fixed handles by name, `hv` = header values by DXF name. Returns the bytes (streams joined).
pub fn encode_header(hh: &HashMap<&'static str, f64>, hv: &Obj, ctx: &HeaderCtx) -> Vec<u8> {
    let ver = version_num(ctx.version);
    let mut w = BitWriter::obj_writer(ctx.version, !crate::w_bits::at_least(ctx.version, "AC1021"));
    if w.r2007 {
        w.size_at = w.bit;
        w.rl_u(0);
    }
    let no_ext = p3v(1e20, 1e20, 1e20);
    let no_ext_max = p3v(-1e20, -1e20, -1e20);
    let jd = |t: f64| {
        let j = julian(t);
        V::Obj(obj! { "day" => j.day, "ms" => j.ms })
    };
    let computed = |name: &str| -> Option<V> {
        Some(match name {
            "UNKNOWN_5" => V::from(if ctx.metric { "m" } else { "" }),
            "EXTMIN" => ctx.extents.map_or_else(|| no_ext.clone(), |e| p3v(e.0.0, e.0.1, e.0.2)),
            "EXTMAX" => ctx.extents.map_or_else(|| no_ext_max.clone(), |e| p3v(e.1.0, e.1.1, e.1.2)),
            "PEXTMIN" => no_ext.clone(),
            "PEXTMAX" => no_ext_max.clone(),
            "TDCREATE" | "TDUPDATE" => jd(ctx.created),
            "HANDSEED" => V::Num(ctx.handseed),
            "FINGERPRINTGUID" => V::from(ctx.guid1.as_str()),
            "VERSIONGUID" => V::from(ctx.guid2.as_str()),
            _ => return None,
        })
    };
    let val = |name: &str| -> Option<V> {
        if let Some(c) = computed(name) {
            return Some(c);
        }
        if name.starts_with("DIM")
            && hv.get(name).is_none()
            && let Some(d) = dim_default(name)
        {
            return Some(d);
        }
        if let Some(v) = nn(hv.get(name)) {
            return Some(v.clone());
        }
        header_default(name).or_else(|| header_def(name))
    };
    for &(name, ty, from, to) in HEADER_VARS {
        if ver < from || ver > to {
            continue;
        }
        let v = val(name);
        let v = v.as_ref();
        match ty {
            HT::B => w.b(truthy(v)),
            HT::Bs => w.bs(f64::from(int32(v))),
            HT::Bl => {
                if name == "FLAGS" {
                    let ps = val("PSTYLEMODE").or(Some(V::Num(1.0)));
                    w.bl(f64::from(29 | 0x200 | 0x800 | if truthy(ps.as_ref()) { 0x2000 } else { 0 }));
                } else {
                    w.bl(f64::from(int32(v)));
                }
            }
            HT::Bll => w.bll(if truthy(v) { to_number(v) } else { 0.0 }),
            HT::Bd => w.bd(num(Some(&V::Num(to_number(v))), 0.0)),
            HT::Rc => w.rc(f64::from(int32(v))),
            HT::Tv => w.tv(&str_or(v, "")),
            HT::T => w.t8(&str_or(v, "")),
            HT::Cmc => match v {
                Some(V::Num(n)) => w.cmc(Some(&V::Obj(obj! { "index" => *n }))),
                Some(x) if x.truthy() => w.cmc(Some(x)),
                _ => w.cmc(Some(&V::Obj(obj! { "index" => 256 }))),
            },
            HT::Bd3 => w.bd3(num(field(v, "x"), 0.0), num(field(v, "y"), 0.0), num(field(v, "z"), 0.0)),
            HT::Rd2 => w.rd2(num(field(v, "x"), 0.0), num(field(v, "y"), 0.0)),
            HT::Td => {
                w.bl(to_number(field(v, "day")));
                w.bl(to_number(field(v, "ms")));
            }
            HT::Ts => {
                w.bl(0.0);
                w.bl(0.0);
            }
            HT::Hs => w.hd(0, crate::w_bits::handle_value(v)),
            HT::H => {
                if name == "CPSNID" {
                    let t = val("CEPSNTYPE").unwrap_or(V::Num(0.0));
                    if int32(Some(&t)) != 3 {
                        continue;
                    }
                }
                let h = header_handle(name).and_then(|k| hh.get(k).copied()).unwrap_or(0.0);
                w.h(if hard_owned(name) { HARD_OWN } else { HARD_PTR }, h);
            }
        }
    }
    let endbit = w.end_data();
    if w.r2007 {
        let at = w.size_at;
        w.patch_rl(at, crate::util::u32_of_usize(endbit));
        w.append_hnd();
    }
    w.bytes()
}
