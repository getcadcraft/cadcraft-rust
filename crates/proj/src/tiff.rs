//! A GeoTIFF reader that answers as geotiff.js 3.0.5 answers (the 0.5 apps read grids and pictures through it):
//! classic and BigTIFF, IFD chains, tags with geotiff.js's "deferred" rule (a value outside the first slice of an IFD
//! is not loaded unless the tag is eager), GeoKeys, origin / resolution / bounding box, strips and tiles, no / LZW /
//! Deflate / PackBits compression with predictors 2 and 3, odd bit depths, float16, nearest and bilinear resampling,
//! and readRGB's colour conversions. JPEG, LERC, zstd and WebP compression are not ported (an "unsupported" error, as
//! geotiff.js gives for a method it lacks). Files are hostile: every read is bounds-checked (a read past the data is
//! the JS's RangeError), IFD chains and counts are capped.

use std::collections::BTreeMap;

type R<T> = Result<T, String>;

const OOB: &str = "Offset is outside the bounds of the DataView";
/// The most IFDs followed (geotiff.js follows a cyclic chain forever).
const MAX_IFDS: usize = 4096;
/// The most cells one read may produce (the JS would try to allocate them). Cells are held as f64 here whatever the
/// typed array kind, so this is a 1 GB budget: 4e8 let a 10-byte hostile header claim a 3.2 GB read (fuzz OOM,
/// 2026-10-21). A 11 000 × 11 000 single-band grid still fits; the largest corpus GeoTIFF is 4000 × 4000.
const MAX_CELLS: f64 = 1.25e8;

/// A JS number used as an index (finite, non-negative, below 2^53).
pub fn idx(x: f64) -> Option<usize> {
    if x.is_finite() && (0.0..9.0e15).contains(&x) { usize::try_from(x as u64).ok() } else { None }
}
/// A count as usize (0 for anything odd).
pub fn to_usize(x: f64) -> usize {
    idx(x.trunc()).unwrap_or(0)
}

/// A typed array's element type (what a value becomes when stored).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Elem {
    U8,
    U8C,
    I8,
    U16,
    I16,
    U32,
    I32,
    F32,
    F64,
    /// a plain JS array (LONG8 / SLONG8 tags)
    Js,
}

fn to_int(x: f64, bits: u32, signed: bool) -> f64 {
    if !x.is_finite() {
        return 0.0;
    }
    let m = 2f64.powi(bits as i32);
    let t = x.trunc().rem_euclid(m);
    if signed && t >= m / 2.0 { t - m } else { t }
}

impl Elem {
    /// The value as the typed array stores it.
    pub fn conv(self, x: f64) -> f64 {
        match self {
            Elem::U8 => to_int(x, 8, false),
            Elem::I8 => to_int(x, 8, true),
            Elem::U16 => to_int(x, 16, false),
            Elem::I16 => to_int(x, 16, true),
            Elem::U32 => to_int(x, 32, false),
            Elem::I32 => to_int(x, 32, true),
            Elem::F32 => f64::from(x as f32),
            Elem::F64 | Elem::Js => x,
            Elem::U8C => {
                if x.is_nan() || x <= 0.0 {
                    0.0
                } else if x >= 255.0 {
                    255.0
                } else {
                    // round half to even
                    let f = x.floor();
                    let d = x - f;
                    if d > 0.5 || (d == 0.5 && f % 2.0 != 0.0) { f + 1.0 } else { f }
                }
            }
        }
    }
    pub fn size(self) -> usize {
        match self {
            Elem::U8 | Elem::U8C | Elem::I8 => 1,
            Elem::U16 | Elem::I16 => 2,
            Elem::U32 | Elem::I32 | Elem::F32 => 4,
            Elem::F64 | Elem::Js => 8,
        }
    }
}

/// A typed array (values as JS numbers).
#[derive(Clone, Debug, PartialEq)]
pub struct TArr {
    pub kind: Elem,
    pub v: Vec<f64>,
}
impl TArr {
    pub fn zeros(kind: Elem, n: usize) -> TArr {
        TArr { kind, v: vec![0.0; n] }
    }
    pub fn len(&self) -> usize {
        self.v.len()
    }
    pub fn is_empty(&self) -> bool {
        self.v.is_empty()
    }
    /// `a[i]` (None = undefined)
    pub fn get(&self, i: usize) -> Option<f64> {
        self.v.get(i).copied()
    }
    /// `a[i] = x` (ignored past the end)
    pub fn set(&mut self, i: usize, x: f64) {
        let k = self.kind;
        if let Some(s) = self.v.get_mut(i) {
            *s = k.conv(x);
        }
    }
    pub fn fill(&mut self, x: f64) {
        let c = self.kind.conv(x);
        self.v.fill(c);
    }
}

/// A tag's value as geotiff.js returns it.
#[derive(Clone, Debug, PartialEq)]
pub enum TagVal {
    Num(f64),
    Arr(TArr),
    Str(String),
}
impl TagVal {
    /// `value[i]` (a number has no elements: undefined)
    pub fn at(&self, i: usize) -> Option<f64> {
        match self {
            TagVal::Arr(a) => a.get(i),
            TagVal::Num(_) => None,
            TagVal::Str(_) => None,
        }
    }
    pub fn as_num(&self) -> Option<f64> {
        match self {
            TagVal::Num(n) => Some(*n),
            _ => None,
        }
    }
    /// A number tag or the first item of an array (`v.length ? v[0]`).
    pub fn first(&self) -> Option<f64> {
        match self {
            TagVal::Num(n) => Some(*n),
            TagVal::Arr(a) => a.get(0),
            TagVal::Str(_) => None,
        }
    }
    pub fn arr(&self) -> Option<&TArr> {
        match self {
            TagVal::Arr(a) => Some(a),
            _ => None,
        }
    }
    /// JS truthiness.
    pub fn truthy(&self) -> bool {
        match self {
            TagVal::Num(n) => *n != 0.0 && !n.is_nan(),
            TagVal::Arr(_) => true,
            TagVal::Str(s) => !s.is_empty(),
        }
    }
}

/// globals.js tag names (the ones the readers ask for) and their flags: (tag, name, isArray, eager).
pub const TAGS: &[(u16, &str, bool, bool)] = &[
    (254, "NewSubfileType", false, true),
    (255, "SubfileType", false, true),
    (256, "ImageWidth", false, true),
    (257, "ImageLength", false, true),
    (258, "BitsPerSample", true, true),
    (259, "Compression", false, true),
    (262, "PhotometricInterpretation", false, true),
    (263, "Threshholding", false, false),
    (264, "CellWidth", false, false),
    (265, "CellLength", false, false),
    (266, "FillOrder", false, false),
    (269, "DocumentName", false, false),
    (270, "ImageDescription", false, false),
    (271, "Make", false, false),
    (272, "Model", false, false),
    (273, "StripOffsets", true, false),
    (274, "Orientation", false, false),
    (277, "SamplesPerPixel", false, true),
    (278, "RowsPerStrip", false, true),
    (279, "StripByteCounts", true, false),
    (280, "MinSampleValue", true, false),
    (281, "MaxSampleValue", true, false),
    (282, "XResolution", false, false),
    (283, "YResolution", false, false),
    (284, "PlanarConfiguration", false, true),
    (285, "PageName", false, false),
    (286, "XPosition", false, false),
    (287, "YPosition", false, false),
    (288, "FreeOffsets", false, false),
    (289, "FreeByteCounts", false, false),
    (290, "GrayResponseUnit", false, false),
    (291, "GrayResponseCurve", true, false),
    (292, "T4Options", false, false),
    (293, "T6Options", false, false),
    (296, "ResolutionUnit", false, false),
    (297, "PageNumber", true, false),
    (301, "TransferFunction", true, false),
    (305, "Software", false, false),
    (306, "DateTime", false, false),
    (315, "Artist", false, false),
    (316, "HostComputer", false, false),
    (317, "Predictor", false, false),
    (318, "WhitePoint", true, false),
    (319, "PrimaryChromaticities", true, false),
    (320, "ColorMap", true, false),
    (321, "HalftoneHints", true, false),
    (322, "TileWidth", false, true),
    (323, "TileLength", false, true),
    (324, "TileOffsets", true, false),
    (325, "TileByteCounts", true, false),
    (332, "InkSet", false, false),
    (333, "InkNames", false, false),
    (334, "NumberOfInks", false, false),
    (336, "DotRange", true, false),
    (337, "TargetPrinter", false, false),
    (338, "ExtraSamples", true, true),
    (339, "SampleFormat", true, true),
    (340, "SMinSampleValue", true, false),
    (341, "SMaxSampleValue", true, false),
    (342, "TransferRange", true, false),
    (512, "JPEGProc", false, false),
    (513, "JPEGInterchangeFormat", false, false),
    (514, "JPEGInterchangeFormatLngth", false, false),
    (515, "JPEGRestartInterval", false, false),
    (517, "JPEGLosslessPredictors", true, false),
    (518, "JPEGPointTransforms", true, false),
    (519, "JPEGQTables", true, false),
    (520, "JPEGDCTables", true, false),
    (521, "JPEGACTables", true, false),
    (529, "YCbCrCoefficients", true, false),
    (530, "YCbCrSubSampling", true, false),
    (531, "YCbCrPositioning", false, false),
    (532, "ReferenceBlackWhite", true, false),
    (33432, "Copyright", false, false),
    (326, "BadFaxLines", false, false),
    (327, "CleanFaxData", false, false),
    (343, "ClipPath", false, false),
    (328, "ConsecutiveBadFaxLines", false, false),
    (433, "Decode", false, false),
    (434, "DefaultImageColor", false, false),
    (346, "Indexed", false, false),
    (347, "JPEGTables", true, true),
    (559, "StripRowCounts", true, false),
    (330, "SubIFDs", true, false),
    (344, "XClipPathUnits", false, false),
    (345, "YClipPathUnits", false, false),
    (37378, "ApertureValue", false, false),
    (40961, "ColorSpace", false, false),
    (36868, "DateTimeDigitized", false, false),
    (36867, "DateTimeOriginal", false, false),
    (34665, "Exif IFD", false, false),
    (36864, "ExifVersion", false, false),
    (33434, "ExposureTime", false, false),
    (41728, "FileSource", false, false),
    (37385, "Flash", false, false),
    (40960, "FlashpixVersion", false, false),
    (33437, "FNumber", false, false),
    (42016, "ImageUniqueID", false, false),
    (37384, "LightSource", false, false),
    (37500, "MakerNote", false, false),
    (37377, "ShutterSpeedValue", false, false),
    (37510, "UserComment", false, false),
    (33723, "IPTC", false, false),
    (34412, "CZ_LSMINFO", false, false),
    (34675, "ICC Profile", false, false),
    (700, "XMP", false, false),
    (42112, "GDAL_METADATA", false, false),
    (42113, "GDAL_NODATA", false, true),
    (34377, "Photoshop", false, false),
    (33550, "ModelPixelScale", true, true),
    (33922, "ModelTiepoint", true, true),
    (34264, "ModelTransformation", true, true),
    (34735, "GeoKeyDirectory", true, true),
    (34736, "GeoDoubleParams", true, true),
    (34737, "GeoAsciiParams", false, true),
    (50674, "LercParameters", false, true),
];

fn tag_def(tag: u16) -> Option<(&'static str, bool, bool)> {
    TAGS.iter().find(|t| t.0 == tag).map(|t| (t.1, t.2, t.3))
}
/// A tag number from its name (`resolveTag`).
pub fn tag_id(name: &str) -> Option<u16> {
    // later registrations win in the JS map: names are unique here
    TAGS.iter().find(|t| t.1 == name).map(|t| t.0)
}

/// globals.js geoKeyNames.
pub const GEO_KEY_NAMES: &[(u16, &str)] = &[
    (1024, "GTModelTypeGeoKey"),
    (1025, "GTRasterTypeGeoKey"),
    (1026, "GTCitationGeoKey"),
    (2048, "GeographicTypeGeoKey"),
    (2049, "GeogCitationGeoKey"),
    (2050, "GeogGeodeticDatumGeoKey"),
    (2051, "GeogPrimeMeridianGeoKey"),
    (2052, "GeogLinearUnitsGeoKey"),
    (2053, "GeogLinearUnitSizeGeoKey"),
    (2054, "GeogAngularUnitsGeoKey"),
    (2055, "GeogAngularUnitSizeGeoKey"),
    (2056, "GeogEllipsoidGeoKey"),
    (2057, "GeogSemiMajorAxisGeoKey"),
    (2058, "GeogSemiMinorAxisGeoKey"),
    (2059, "GeogInvFlatteningGeoKey"),
    (2060, "GeogAzimuthUnitsGeoKey"),
    (2061, "GeogPrimeMeridianLongGeoKey"),
    (2062, "GeogTOWGS84GeoKey"),
    (3072, "ProjectedCSTypeGeoKey"),
    (3073, "PCSCitationGeoKey"),
    (3074, "ProjectionGeoKey"),
    (3075, "ProjCoordTransGeoKey"),
    (3076, "ProjLinearUnitsGeoKey"),
    (3077, "ProjLinearUnitSizeGeoKey"),
    (3078, "ProjStdParallel1GeoKey"),
    (3079, "ProjStdParallel2GeoKey"),
    (3080, "ProjNatOriginLongGeoKey"),
    (3081, "ProjNatOriginLatGeoKey"),
    (3082, "ProjFalseEastingGeoKey"),
    (3083, "ProjFalseNorthingGeoKey"),
    (3084, "ProjFalseOriginLongGeoKey"),
    (3085, "ProjFalseOriginLatGeoKey"),
    (3086, "ProjFalseOriginEastingGeoKey"),
    (3087, "ProjFalseOriginNorthingGeoKey"),
    (3088, "ProjCenterLongGeoKey"),
    (3089, "ProjCenterLatGeoKey"),
    (3090, "ProjCenterEastingGeoKey"),
    (3091, "ProjCenterNorthingGeoKey"),
    (3092, "ProjScaleAtNatOriginGeoKey"),
    (3093, "ProjScaleAtCenterGeoKey"),
    (3094, "ProjAzimuthAngleGeoKey"),
    (3095, "ProjStraightVertPoleLongGeoKey"),
    (3096, "ProjRectifiedGridAngleGeoKey"),
    (4096, "VerticalCSTypeGeoKey"),
    (4097, "VerticalCitationGeoKey"),
    (4098, "VerticalDatumGeoKey"),
    (4099, "VerticalUnitsGeoKey"),
];

/// One GeoKey value: a number, text, a short list, or undefined.
#[derive(Clone, Debug, PartialEq)]
pub enum KeyVal {
    Undef,
    Num(f64),
    Str(String),
    Arr(Vec<f64>),
}

fn field_size(ft: u16) -> R<usize> {
    Ok(match ft {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 | 13 => 4,
        5 | 10 | 12 | 16 | 17 | 18 => 8,
        _ => return Err(format!("Invalid field type: {ft}")),
    })
}
fn field_elem(ft: u16) -> R<Elem> {
    Ok(match ft {
        1 | 2 | 7 => Elem::U8,
        6 => Elem::I8,
        3 => Elem::U16,
        8 => Elem::I16,
        4 | 13 | 5 => Elem::U32,
        9 | 10 => Elem::I32,
        11 => Elem::F32,
        12 => Elem::F64,
        16..=18 => Elem::Js,
        _ => return Err(format!("Invalid field type: {ft}")),
    })
}

/// The file's bytes with its byte order.
#[derive(Clone, Debug)]
struct Bytes<'a> {
    b: &'a [u8],
    le: bool,
}
impl Bytes<'_> {
    fn get<const N: usize>(&self, off: f64) -> R<[u8; N]> {
        let o = idx(off).ok_or(OOB)?;
        let s = o.checked_add(N).and_then(|e| self.b.get(o..e)).ok_or(OOB)?;
        let mut a = [0u8; N];
        a.copy_from_slice(s);
        Ok(a)
    }
    fn u8(&self, off: f64) -> R<f64> {
        Ok(f64::from(self.get::<1>(off)?[0]))
    }
    fn i8(&self, off: f64) -> R<f64> {
        Ok(f64::from(i8::from_le_bytes(self.get::<1>(off)?)))
    }
    fn u16(&self, off: f64) -> R<f64> {
        let a = self.get::<2>(off)?;
        Ok(f64::from(if self.le { u16::from_le_bytes(a) } else { u16::from_be_bytes(a) }))
    }
    fn i16(&self, off: f64) -> R<f64> {
        let a = self.get::<2>(off)?;
        Ok(f64::from(if self.le { i16::from_le_bytes(a) } else { i16::from_be_bytes(a) }))
    }
    fn u32(&self, off: f64) -> R<f64> {
        let a = self.get::<4>(off)?;
        Ok(f64::from(if self.le { u32::from_le_bytes(a) } else { u32::from_be_bytes(a) }))
    }
    fn i32(&self, off: f64) -> R<f64> {
        let a = self.get::<4>(off)?;
        Ok(f64::from(if self.le { i32::from_le_bytes(a) } else { i32::from_be_bytes(a) }))
    }
    fn f32(&self, off: f64) -> R<f64> {
        let a = self.get::<4>(off)?;
        Ok(f64::from(if self.le { f32::from_le_bytes(a) } else { f32::from_be_bytes(a) }))
    }
    fn f64(&self, off: f64) -> R<f64> {
        let a = self.get::<8>(off)?;
        Ok(if self.le { f64::from_le_bytes(a) } else { f64::from_be_bytes(a) })
    }
    fn u64(&self, off: f64) -> R<f64> {
        let left = self.u32(off)?;
        let right = self.u32(off + 4.0)?;
        let combined = if self.le { left + 4294967296.0 * right } else { 4294967296.0 * left + right };
        if combined > 9007199254740991.0 {
            return Err(format!(
                "{} exceeds MAX_SAFE_INTEGER. Precision may be lost. Please report if you get this message to https://github.com/geotiffjs/geotiff.js/issues",
                cc_cad::js::num_to_string(combined)
            ));
        }
        Ok(combined)
    }
    /// readInt64 (geotiff.js reads it at the absolute offset, without the slice's start: the whole file is one slice
    /// here, so that is the same).
    fn i64(&self, off: f64) -> R<f64> {
        let o = idx(off).ok_or(OOB)?;
        let at = |i: usize| self.b.get(o.saturating_add(i)).copied().ok_or(OOB);
        let neg = at(if self.le { 7 } else { 0 })? & 0x80 > 0;
        let mut value = 0.0f64;
        let mut carrying = true;
        for i in 0..8usize {
            let mut byte = at(if self.le { i } else { 7 - i })?;
            if neg {
                if carrying {
                    if byte != 0 {
                        byte = !(byte.wrapping_sub(1));
                        carrying = false;
                    }
                } else {
                    byte = !byte;
                }
            }
            value += f64::from(byte) * 256f64.powi(i as i32);
        }
        Ok(if neg { -value } else { value })
    }
    fn read(&self, ft: u16, off: f64) -> R<f64> {
        match ft {
            1 | 2 | 7 => self.u8(off),
            6 => self.i8(off),
            3 => self.u16(off),
            8 => self.i16(off),
            4 | 13 | 5 => self.u32(off),
            9 | 10 => self.i32(off),
            11 => self.f32(off),
            12 => self.f64(off),
            16 | 18 => self.u64(off),
            17 => self.i64(off),
            _ => Err(format!("Invalid field type: {ft}")),
        }
    }
}

/// `getValues(…)`: count values of a field type at an offset.
fn get_values(bytes: &Bytes, ft: u16, count: f64, offset: f64, is_array: bool) -> R<TagVal> {
    let size = field_size(ft)? as f64;
    let elem = field_elem(ft)?;
    let rational = ft == 5 || ft == 10;
    let n = if rational { count * 2.0 } else { count };
    // the JS allocates first; a count the file cannot hold fails on the first read past its end
    if !(n * size <= bytes.b.len() as f64 * 2.0 + 16.0) {
        return Err(OOB.into());
    }
    let mut vals = TArr::zeros(elem, to_usize(n));
    if !rational {
        let mut i = 0.0;
        while i < count {
            let v = bytes.read(ft, offset + i * size)?;
            vals.set(to_usize(i), v);
            i += 1.0;
        }
    } else {
        // the JS steps i by 2 over `count` (a quirk): the first `count` words hold count/2 rationals
        let mut i = 0.0;
        while i < count {
            let a = bytes.read(ft, offset + i * size)?;
            let b = bytes.read(ft, offset + (i * size + 4.0))?;
            vals.set(to_usize(i), a);
            vals.set(to_usize(i + 1.0), b);
            i += 2.0;
        }
    }
    if ft == 2 {
        let raw: Vec<u8> = vals.v.iter().map(|&x| x as u8).collect();
        return Ok(TagVal::Str(String::from_utf8_lossy(&raw).into_owned()));
    }
    if count == 1.0 && !is_array && !rational {
        return Ok(TagVal::Num(vals.get(0).unwrap_or(f64::NAN)));
    }
    if elem == Elem::Js {
        vals.kind = Elem::Js;
    }
    Ok(TagVal::Arr(vals))
}

/// A field that geotiff.js leaves deferred (its value outside the IFD's first slice).
#[derive(Clone, Debug)]
struct Deferred {
    ft: u16,
    count: f64,
    offset: f64,
    is_array: bool,
}

/// An image file directory.
#[derive(Clone, Debug)]
pub struct Ifd {
    actual: BTreeMap<u16, TagVal>,
    deferred: BTreeMap<u16, Deferred>,
    /// deferred arrays (isArray tags): loaded whole or by index
    deferred_arrays: BTreeMap<u16, Deferred>,
    pub next_ifd: f64,
}

/// A parsed GeoTIFF.
#[derive(Clone, Debug)]
pub struct Tiff {
    data: Vec<u8>,
    le: bool,
    /// BigTIFF (version 43)
    pub big: bool,
    ifds: Vec<Ifd>,
    /// the error that ended the IFD chain (a non-index error: the JS's getImageCount rethrows it)
    chain_error: Option<String>,
}

fn parse_ifd(bytes: &Bytes, big: bool, offset: f64) -> R<Ifd> {
    let entry = if big { 20.0 } else { 12.0 };
    let off_size = if big { 8.0 } else { 2.0 };
    let file_len = bytes.b.len() as f64;
    // the first slice: 1024 (4048 for BigTIFF) bytes from the IFD, clamped to the file
    let fallback = if big { 4048.0 } else { 1024.0 };
    let clamp = |o: f64, l: f64| -> (f64, f64) {
        let s = o.max(0.0).min(file_len);
        let e = (o + l).max(0.0).min(file_len);
        (s, e.max(s))
    };
    let (mut s0, mut s1) = clamp(offset, fallback);
    // reads go through the slice: anything outside it is a RangeError
    let in_slice = |s0: f64, s1: f64, o: f64, n: f64| o >= s0 && o + n <= s1;
    let read_in = |s0: f64, s1: f64, ft: u16, o: f64| -> R<f64> {
        let n = field_size(ft).unwrap_or(1) as f64;
        if !in_slice(s0, s1, o, n) {
            return Err(OOB.into());
        }
        bytes.read(ft, o)
    };
    let num = if big { read_in(s0, s1, 16, offset)? } else { read_in(s0, s1, 3, offset)? };
    let byte_size = num * (entry + if big { 16.0 } else { 6.0 });
    let covers = |s0: f64, s1: f64, o: f64, n: f64| s0 <= o && s1 >= o + n;
    if !covers(s0, s1, offset, byte_size) {
        let c = clamp(offset, byte_size);
        s0 = c.0;
        s1 = c.1;
    }
    let mut ifd = Ifd { actual: BTreeMap::new(), deferred: BTreeMap::new(), deferred_arrays: BTreeMap::new(), next_ifd: 0.0 };
    let mut i = offset + if big { 8.0 } else { 2.0 };
    let mut k = 0.0;
    while k < num {
        let tag_f = read_in(s0, s1, 3, i)?;
        let tag = tag_f as u16;
        let ft = read_in(s0, s1, 3, i + 2.0)? as u16;
        let count = if big { read_in(s0, s1, 16, i + 4.0)? } else { read_in(s0, s1, 4, i + 4.0)? };
        let size = field_size(ft)? as f64;
        let value_off = i + if big { 12.0 } else { 8.0 };
        let def = tag_def(tag);
        let is_array = def.is_some_and(|d| d.1);
        let eager = def.is_some_and(|d| d.2);
        let slice_bytes = Bytes { b: bytes.b.get(to_usize(s0)..to_usize(s1)).unwrap_or(&[]), le: bytes.le };
        let rel = |o: f64| o - s0;
        if size * count <= if big { 8.0 } else { 4.0 } {
            let v = get_values(&slice_bytes, ft, count, rel(value_off), is_array)?;
            ifd.actual.insert(tag, v);
        } else {
            let actual_off = if big { read_in(s0, s1, 16, value_off)? } else { read_in(s0, s1, 4, value_off)? };
            let length = size * count;
            if covers(s0, s1, actual_off, length) {
                let v = get_values(&slice_bytes, ft, count, rel(actual_off), is_array)?;
                ifd.actual.insert(tag, v);
            } else if eager {
                let (e0, e1) = clamp(actual_off, length);
                let eb = Bytes { b: bytes.b.get(to_usize(e0)..to_usize(e1)).unwrap_or(&[]), le: bytes.le };
                let v = get_values(&eb, ft, count, actual_off - e0, is_array)?;
                ifd.actual.insert(tag, v);
            } else if is_array {
                ifd.deferred_arrays.insert(tag, Deferred { ft, count, offset: actual_off, is_array });
            } else {
                ifd.deferred.insert(tag, Deferred { ft, count, offset: actual_off, is_array });
            }
        }
        i += entry;
        k += 1.0;
    }
    let next_at = offset + off_size + entry * num;
    ifd.next_ifd = if big { read_in(s0, s1, 16, next_at)? } else { read_in(s0, s1, 4, next_at)? };
    Ok(ifd)
}

impl Tiff {
    /// `GeoTIFF.fromSource` + reading every IFD (geotiff.js reads them lazily; the result is the same).
    pub fn from_bytes(data: &[u8]) -> R<Tiff> {
        let probe = Bytes { b: data, le: false };
        let bom = probe.u16(0.0)?;
        let le = if bom == f64::from(0x4949u16) {
            true
        } else if bom == f64::from(0x4D4Du16) {
            false
        } else {
            return Err("Invalid byte order value.".into());
        };
        let b = Bytes { b: data, le };
        let magic = b.u16(2.0)?;
        let big = if magic == 42.0 {
            false
        } else if magic == 43.0 {
            if b.u16(4.0)? != 8.0 {
                return Err("Unsupported offset byte-size.".into());
            }
            true
        } else {
            return Err("Invalid magic number.".into());
        };
        let first = if big { b.u64(8.0)? } else { b.u32(4.0)? };
        let mut t = Tiff { data: data.to_vec(), le, big, ifds: Vec::new(), chain_error: None };
        let mut off = first;
        for _ in 0..MAX_IFDS {
            let bytes = Bytes { b: &t.data, le };
            match parse_ifd(&bytes, big, off) {
                Ok(ifd) => {
                    let next = ifd.next_ifd;
                    t.ifds.push(ifd);
                    if next == 0.0 {
                        break;
                    }
                    off = next;
                }
                Err(e) => {
                    t.chain_error = Some(e);
                    break;
                }
            }
        }
        Ok(t)
    }
    /// `getImageCount()` (Err = the error reading the chain threw).
    pub fn image_count_checked(&self) -> R<usize> {
        match &self.chain_error {
            Some(e) => Err(e.clone()),
            None => Ok(self.ifds.len()),
        }
    }
    pub fn image_count(&self) -> usize {
        self.ifds.len()
    }
    /// `getImage(index)`.
    pub fn image(&self, index: usize) -> R<Image<'_>> {
        let Some(ifd) = self.ifds.get(index) else {
            if index == self.ifds.len()
                && let Some(e) = &self.chain_error
            {
                return Err(e.clone());
            }
            return Err(format!("No image at index {index}"));
        };
        Image::new(self, ifd)
    }
}

/// Read options (`readRasters` / `readRGB`).
#[derive(Clone, Debug, Default)]
pub struct ReadOptions {
    pub window: Option<[f64; 4]>,
    pub samples: Vec<usize>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub resample: Option<String>,
    pub fill: Option<f64>,
    pub interleave: bool,
    pub enable_alpha: bool,
}

/// The result of a read: one array per sample (or one interleaved array), with width / height.
#[derive(Clone, Debug, PartialEq)]
pub struct Rasters {
    pub bands: Vec<Vec<f64>>,
    pub width: f64,
    pub height: f64,
    pub kinds: Vec<Elem>,
}

/// A GeoTIFFImage.
#[derive(Clone, Debug)]
pub struct Image<'a> {
    tiff: &'a Tiff,
    pub ifd: &'a Ifd,
    pub is_tiled: bool,
    pub planar: f64,
}

/// `JSON-ish` view of a value for messages.
fn tag_name(tag: u16) -> String {
    tag_def(tag).map_or_else(|| format!("Tag{tag}"), |d| d.0.to_string())
}

impl<'a> Image<'a> {
    fn new(tiff: &'a Tiff, ifd: &'a Ifd) -> R<Image<'a>> {
        let mut img = Image { tiff, ifd, is_tiled: false, planar: 1.0 };
        img.is_tiled = !img.has_tag(273);
        let pc = img.get_value(284)?.and_then(|v| v.as_num().or_else(|| v.first()));
        let pc = pc.unwrap_or(1.0);
        if pc != 1.0 && pc != 2.0 {
            return Err("Invalid planar configuration.".into());
        }
        img.planar = pc;
        Ok(img)
    }
    pub fn has_tag(&self, tag: u16) -> bool {
        self.ifd.actual.contains_key(&tag) || self.ifd.deferred.contains_key(&tag) || self.ifd.deferred_arrays.contains_key(&tag)
    }
    pub fn has(&self, name: &str) -> bool {
        tag_id(name).is_some_and(|t| self.has_tag(t))
    }
    /// `fileDirectory.getValue(tag)`: Ok(None) = undefined; Err = "deferred".
    pub fn get_value(&self, tag: u16) -> R<Option<TagVal>> {
        if self.ifd.deferred.contains_key(&tag) || self.ifd.deferred_arrays.contains_key(&tag) {
            return Err(format!("Field '{}' ({tag}) is deferred. Use loadValue() to load it asynchronously.", tag_name(tag)));
        }
        Ok(self.ifd.actual.get(&tag).cloned())
    }
    /// `getValue(name)`.
    pub fn value(&self, name: &str) -> R<Option<TagVal>> {
        match tag_id(name) {
            Some(t) => self.get_value(t),
            None => Ok(None),
        }
    }
    fn gv(&self, tag: u16) -> Option<TagVal> {
        self.get_value(tag).ok().flatten()
    }
    /// `loadValue(tag)`.
    pub fn load_value(&self, tag: u16) -> R<Option<TagVal>> {
        if let Some(v) = self.ifd.actual.get(&tag) {
            return Ok(Some(v.clone()));
        }
        let d = self.ifd.deferred.get(&tag).or_else(|| self.ifd.deferred_arrays.get(&tag));
        match d {
            Some(d) => {
                let len = field_size(d.ft)? as f64 * d.count;
                let file_len = self.tiff.data.len() as f64;
                let s = d.offset.max(0.0).min(file_len);
                let e = (d.offset + len).max(0.0).min(file_len).max(s);
                let sb = Bytes { b: self.tiff.data.get(to_usize(s)..to_usize(e)).unwrap_or(&[]), le: self.tiff.le };
                // deferred arrays are read little-endian (geotiff.js passes `true`), deferred fields with the file's order
                let sb = if self.ifd.deferred_arrays.contains_key(&tag) { Bytes { b: sb.b, le: true } } else { sb };
                let is_array = self.ifd.deferred_arrays.contains_key(&tag) || d.is_array;
                get_values(&sb, d.ft, d.count, d.offset - s, is_array).map(Some)
            }
            None => Ok(None),
        }
    }
    /// `loadValueIndexed(tag, index)`.
    pub fn load_value_indexed(&self, tag: u16, index: f64) -> R<Option<f64>> {
        if let Some(v) = self.ifd.actual.get(&tag) {
            return Ok(idx(index).and_then(|i| v.at(i)));
        }
        if let Some(d) = self.ifd.deferred_arrays.get(&tag) {
            if index < 0.0 || index >= d.count {
                return Err(format!(
                    "Index {} out of bounds for length {}",
                    cc_cad::js::num_to_string(index),
                    cc_cad::js::num_to_string(d.count)
                ));
            }
            let size = field_size(d.ft)? as f64;
            let off = d.offset + index * size;
            let file_len = self.tiff.data.len() as f64;
            let s = off.max(0.0).min(file_len);
            let e = (off + size).max(0.0).min(file_len).max(s);
            let sb = Bytes { b: self.tiff.data.get(to_usize(s)..to_usize(e)).unwrap_or(&[]), le: true };
            return sb.read(d.ft, off - s).map(Some);
        }
        if self.has_tag(tag) {
            let v = self.load_value(tag)?;
            if let Some(v) = v
                && v.truthy()
                && !matches!(v, TagVal::Num(_))
            {
                return Ok(idx(index).and_then(|i| v.at(i)));
            }
        }
        Ok(None)
    }

    pub fn width(&self) -> f64 {
        self.gv(256).and_then(|v| v.as_num().or_else(|| v.first())).filter(|v| *v != 0.0 && !v.is_nan()).unwrap_or(0.0)
    }
    pub fn height(&self) -> f64 {
        self.gv(257).and_then(|v| v.as_num().or_else(|| v.first())).filter(|v| *v != 0.0 && !v.is_nan()).unwrap_or(0.0)
    }
    pub fn samples_per_pixel(&self) -> f64 {
        self.gv(277).and_then(|v| v.as_num().or_else(|| v.first())).unwrap_or(1.0)
    }
    pub fn tile_width(&self) -> f64 {
        if self.is_tiled { self.gv(322).and_then(|v| v.first()).filter(|v| *v != 0.0 && !v.is_nan()).unwrap_or(0.0) } else { self.width() }
    }
    pub fn tile_height(&self) -> f64 {
        if self.is_tiled {
            return self.gv(323).and_then(|v| v.first()).filter(|v| *v != 0.0 && !v.is_nan()).unwrap_or(0.0);
        }
        let rps = if self.has_tag(278) { self.gv(278).and_then(|v| v.first()) } else { None };
        match rps {
            Some(r) if r != 0.0 && !r.is_nan() => crate::math::min(r, self.height()),
            _ => self.height(),
        }
    }
    fn block_height(&self, y: f64) -> f64 {
        if self.is_tiled || (y + 1.0) * self.tile_height() <= self.height() {
            self.tile_height()
        } else {
            self.height() - y * self.tile_height()
        }
    }
    fn bits(&self) -> Vec<f64> {
        match self.gv(258) {
            Some(TagVal::Arr(a)) => a.v.clone(),
            Some(TagVal::Num(n)) => vec![n],
            _ => Vec::new(),
        }
    }
    fn bytes_per_pixel(&self) -> R<f64> {
        let b = self.bits();
        let mut s = 0.0;
        for i in 0..b.len() {
            s += self.sample_byte_size(i)?;
        }
        Ok(s)
    }
    fn sample_byte_size(&self, i: usize) -> R<f64> {
        let b = self.bits();
        match b.get(i) {
            Some(v) => Ok((v / 8.0).ceil()),
            None => Err(format!("Sample index {i} is out of range.")),
        }
    }
    pub fn sample_format(&self, i: usize) -> f64 {
        match self.gv(339) {
            Some(TagVal::Arr(a)) => a.get(i).unwrap_or(f64::NAN),
            // a SampleFormat that is not an array: `value[i]` of a number is undefined
            Some(TagVal::Num(_) | TagVal::Str(_)) => f64::NAN,
            None => 1.0,
        }
    }
    pub fn bits_per_sample(&self, i: usize) -> f64 {
        match self.gv(258) {
            Some(TagVal::Arr(a)) => a.get(i).unwrap_or(f64::NAN),
            Some(_) => f64::NAN,
            None => 0.0,
        }
    }
    fn array_kind(format: f64, bits: f64) -> R<Elem> {
        let k = if format == 1.0 {
            if bits <= 8.0 {
                Some(Elem::U8)
            } else if bits <= 16.0 {
                Some(Elem::U16)
            } else if bits <= 32.0 {
                Some(Elem::U32)
            } else {
                None
            }
        } else if format == 2.0 {
            if bits == 8.0 {
                Some(Elem::I8)
            } else if bits == 16.0 {
                Some(Elem::I16)
            } else if bits == 32.0 {
                Some(Elem::I32)
            } else {
                None
            }
        } else if format == 3.0 {
            if bits == 16.0 || bits == 32.0 {
                Some(Elem::F32)
            } else if bits == 64.0 {
                Some(Elem::F64)
            } else {
                None
            }
        } else {
            None
        };
        k.ok_or_else(|| "Unsupported data format/bitsPerSample".to_string())
    }

    /// `getGeoKeys()` → (name, value) in insertion order; Ok(None) = null (no GeoKeyDirectory).
    pub fn geo_keys(&self) -> R<Option<Vec<(String, KeyVal)>>> {
        let raw = self.get_value(34735)?;
        let Some(raw) = raw.filter(TagVal::truthy) else { return Ok(None) };
        let at = |i: f64| idx(i).and_then(|k| raw.at(k));
        let mut out: Vec<(String, KeyVal)> = Vec::new();
        let n = at(3.0).unwrap_or(f64::NAN) * 4.0;
        let mut i = 4.0;
        let mut guard = 0u32;
        while i <= n && guard < 70_000 {
            let key = at(i)
                .and_then(|id| GEO_KEY_NAMES.iter().find(|g| f64::from(g.0) == id).map(|g| g.1.to_string()))
                .unwrap_or_else(|| "undefined".into());
            let location = at(i + 1.0).filter(|v| *v != 0.0 && !v.is_nan());
            let count = at(i + 2.0);
            let offset = at(i + 3.0);
            let value = match location {
                None => offset.map_or(KeyVal::Undef, KeyVal::Num),
                Some(loc) => {
                    let v = self.get_value(loc as u16)?;
                    match v {
                        None => return Err(format!("Could not get value of geoKey '{key}'.")),
                        Some(TagVal::Str(s)) => {
                            let o = offset.unwrap_or(f64::NAN);
                            let c = count.unwrap_or(f64::NAN);
                            KeyVal::Str(js_substring(&s, o, o + c - 1.0))
                        }
                        Some(TagVal::Arr(a)) => {
                            let o = offset.unwrap_or(f64::NAN);
                            let c = count.unwrap_or(f64::NAN);
                            let sub = js_subarray(&a.v, o, o + c);
                            if c == 1.0 { sub.first().map_or(KeyVal::Undef, |x| KeyVal::Num(*x)) } else { KeyVal::Arr(sub) }
                        }
                        Some(TagVal::Num(nv)) => KeyVal::Num(nv),
                    }
                }
            };
            match out.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = value,
                None => out.push((key, value)),
            }
            i += 4.0;
            guard = guard.saturating_add(1);
        }
        Ok(Some(out))
    }
    pub fn model_pixel_scale(&self) -> Option<Vec<f64>> {
        self.gv(33550).and_then(|v| v.arr().map(|a| a.v.clone()))
    }
    fn arr_tag(&self, tag: u16) -> R<Option<Vec<f64>>> {
        Ok(self.get_value(tag)?.and_then(|v| match v {
            TagVal::Arr(a) => Some(a.v),
            TagVal::Num(n) => Some(vec![n]),
            TagVal::Str(_) => None,
        }))
    }
    pub fn origin(&self) -> R<[f64; 3]> {
        let tie = self.arr_tag(33922)?;
        let mt = self.arr_tag(34264)?;
        let g = |v: &Vec<f64>, i: usize| v.get(i).copied().unwrap_or(f64::NAN);
        if let Some(t) = tie.as_ref().filter(|t| t.len() == 6) {
            return Ok([g(t, 3), g(t, 4), g(t, 5)]);
        }
        if let Some(m) = &mt {
            return Ok([g(m, 3), g(m, 7), g(m, 11)]);
        }
        Err("The image does not have an affine transformation.".into())
    }
    pub fn resolution(&self) -> R<[f64; 3]> {
        let mps = self.arr_tag(33550)?;
        let mt = self.arr_tag(34264)?;
        let g = |v: &Vec<f64>, i: usize| v.get(i).copied().unwrap_or(f64::NAN);
        if let Some(s) = &mps {
            return Ok([g(s, 0), -g(s, 1), g(s, 2)]);
        }
        if let Some(m) = &mt {
            if g(m, 1) == 0.0 && g(m, 4) == 0.0 {
                return Ok([g(m, 0), -g(m, 5), g(m, 10)]);
            }
            return Ok([(g(m, 0) * g(m, 0) + g(m, 4) * g(m, 4)).sqrt(), -(g(m, 1) * g(m, 1) + g(m, 5) * g(m, 5)).sqrt(), g(m, 10)]);
        }
        Err("The image does not have an affine transformation.".into())
    }
    /// `getBoundingBox(tilegrid)`.
    pub fn bounding_box(&self, tilegrid: bool) -> R<[f64; 4]> {
        let h = self.height();
        let w = self.width();
        let mt = self.arr_tag(34264)?;
        if let Some(m) = mt.filter(|_| !tilegrid) {
            let g = |i: usize| m.get(i).copied().unwrap_or(f64::NAN);
            let (a, b, d, e, f, hh) = (g(0), g(1), g(3), g(4), g(5), g(7));
            let corners = [(0.0, 0.0), (0.0, h), (w, 0.0), (w, h)];
            let xs: Vec<f64> = corners.iter().map(|(i, j)| d + a * i + b * j).collect();
            let ys: Vec<f64> = corners.iter().map(|(i, j)| hh + e * i + f * j).collect();
            return Ok([
                cc_cad::js::min_of(xs.iter().copied()),
                cc_cad::js::min_of(ys.iter().copied()),
                cc_cad::js::max_of(xs.iter().copied()),
                cc_cad::js::max_of(ys.iter().copied()),
            ]);
        }
        let o = self.origin()?;
        let r = self.resolution()?;
        let x1 = o[0];
        let y1 = o[1];
        let x2 = x1 + r[0] * w;
        let y2 = y1 + r[1] * h;
        Ok([crate::math::min(x1, x2), crate::math::min(y1, y2), crate::math::max(x1, x2), crate::math::max(y1, y2)])
    }
    /// `getGDALNoData()`.
    pub fn gdal_nodata(&self) -> Option<f64> {
        if !self.has_tag(42113) {
            return None;
        }
        match self.gv(42113) {
            Some(TagVal::Str(s)) if !s.is_empty() => {
                let n = s.encode_utf16().count();
                Some(cc_cad::js::str_to_number(&cc_cad::js::slice16(&s, n.saturating_sub(1))))
            }
            _ => None,
        }
    }

    fn decode_block(&self, raw: &[u8], compression: f64, params: &DecoderParams) -> R<Vec<u8>> {
        let decoded = if compression == 1.0 || compression.is_nan() {
            raw.to_vec()
        } else if compression == 5.0 {
            lzw_decompress(raw)?
        } else if compression == 8.0 || compression == 32946.0 {
            inflate_zlib(raw)?
        } else if compression == 32773.0 {
            packbits(raw)?
        } else if compression == 6.0 {
            return Err("old style JPEG compression is not supported.".into());
        } else if [7.0, 34887.0, 50000.0, 50001.0].contains(&compression) {
            return Err(format!("Compression {} is not ported to the Rust fork", cc_cad::js::num_to_string(compression)));
        } else {
            return Err(format!("Unknown compression method identifier: {}", cc_cad::js::num_to_string(compression)));
        };
        if params.predictor != 1.0 {
            return apply_predictor(decoded, params);
        }
        Ok(decoded)
    }

    fn decoder_params(&self) -> R<DecoderParams> {
        let tw = if self.is_tiled { self.load_value(322)? } else { self.load_value(256)? };
        let th = if self.is_tiled {
            self.load_value(323)?
        } else {
            match self.load_value(278)? {
                Some(v) if v.truthy() => Some(v),
                _ => self.load_value(257)?,
            }
        };
        let pc = self.load_value(284)?;
        let bits = self.load_value(258)?;
        let pred = self.load_value(317)?.filter(TagVal::truthy).and_then(|v| v.first()).unwrap_or(1.0);
        Ok(DecoderParams {
            tile_width: tw.and_then(|v| v.first()).unwrap_or(f64::NAN),
            tile_height: th.and_then(|v| v.first()).unwrap_or(f64::NAN),
            planar: pc.and_then(|v| v.first()),
            bits: match bits {
                Some(TagVal::Arr(a)) => a.v,
                Some(TagVal::Num(n)) => vec![n],
                _ => vec![f64::NAN],
            },
            predictor: pred,
        })
    }

    fn tile_or_strip(&self, x: f64, y: f64, sample: f64, compression: f64, params: &DecoderParams) -> R<Vec<u8>> {
        let per_row = (self.width() / self.tile_width()).ceil();
        let per_col = (self.height() / self.tile_height()).ceil();
        let index = if self.planar == 1.0 { y * per_row + x } else { sample * per_row * per_col + y * per_row + x };
        let (otag, ctag) = if self.is_tiled { (324, 325) } else { (273, 279) };
        let offset = self.load_value_indexed(otag, index)?.unwrap_or(f64::NAN);
        let count = self.load_value_indexed(ctag, index)?.unwrap_or(f64::NAN);
        if count == 0.0 {
            let npx = self.block_height(y) * self.tile_width();
            let bpp = if self.planar == 2.0 { self.sample_byte_size(to_usize(sample))? } else { self.bytes_per_pixel()? };
            let kind = Self::array_kind(self.sample_format(to_usize(sample)), self.bits_per_sample(to_usize(sample)))?;
            let nbytes = to_usize(npx * bpp);
            if nbytes as f64 > MAX_CELLS * 8.0 {
                return Err("Array buffer allocation failed".into());
            }
            let fill = self.gdal_nodata().filter(|v| *v != 0.0 && !v.is_nan()).unwrap_or(0.0);
            let n = nbytes / kind.size().max(1);
            let mut out = vec![0u8; nbytes];
            let val = kind.conv(fill);
            for i in 0..n {
                write_native(&mut out, kind, i, val);
            }
            return Ok(out);
        }
        let file_len = self.tiff.data.len() as f64;
        let start = if offset.is_nan() { 0.0 } else { offset.max(0.0).min(file_len) };
        let end = if offset.is_nan() || count.is_nan() { start } else { (offset + count).max(0.0).min(file_len).max(start) };
        let raw = self.tiff.data.get(to_usize(start)..to_usize(end)).unwrap_or(&[]);
        let mut data = self.decode_block(raw, compression, params)?;
        let fmt = self.sample_format(0);
        let bits = self.bits_per_sample(0);
        if needs_normalization(fmt, bits) {
            data = normalize_array(&data, fmt, self.planar, self.samples_per_pixel(), bits, self.tile_width(), self.block_height(y))?;
        }
        Ok(data)
    }

    /// `readRasters(options)`.
    pub fn read_rasters(&self, o: &ReadOptions) -> R<Rasters> {
        let window = o.window.unwrap_or([0.0, 0.0, self.width(), self.height()]);
        if window[0] > window[2] || window[1] > window[3] {
            return Err("Invalid subsets".into());
        }
        let ww = window[2] - window[0];
        let wh = window[3] - window[1];
        let npx = ww * wh;
        let spp = self.samples_per_pixel();
        let mut samples = o.samples.clone();
        if samples.is_empty() {
            let mut i = 0.0;
            while i < spp && i < 65536.0 {
                samples.push(to_usize(i));
                i += 1.0;
            }
        } else {
            for &s in &samples {
                if s as f64 >= spp {
                    return Err(format!("Invalid sample index '{s}'."));
                }
            }
        }
        if npx * samples.len().max(1) as f64 > MAX_CELLS {
            return Err("Invalid typed array length".into());
        }
        let n = to_usize(npx);
        let mut bands: Vec<TArr> = Vec::new();
        let mut inter: Option<TArr> = None;
        if o.interleave {
            let fmt = match self.gv(339) {
                Some(TagVal::Arr(a)) => cc_cad::js::max_of(a.v.iter().copied()),
                Some(TagVal::Num(nv)) => nv,
                _ => 1.0,
            };
            if fmt != 1.0 && fmt != 2.0 && fmt != 3.0 {
                return Err("Unsupported sample format for interleaved data. Must be 1, 2, or 3.".into());
            }
            let bits = match self.gv(258) {
                Some(TagVal::Arr(a)) => cc_cad::js::max_of(a.v.iter().copied()),
                Some(TagVal::Num(nv)) => nv,
                _ => 8.0,
            };
            let mut a = TArr::zeros(Self::array_kind(fmt, bits)?, n.saturating_mul(samples.len()));
            if let Some(f) = o.fill.filter(|v| *v != 0.0 && !v.is_nan()) {
                a.fill(f);
            }
            inter = Some(a);
        } else {
            for &s in &samples {
                let mut a = TArr::zeros(Self::array_kind(self.sample_format(s), self.bits_per_sample(s))?, n);
                if let Some(f) = o.fill.filter(|v| *v != 0.0 && !v.is_nan()) {
                    a.fill(f);
                }
                bands.push(a);
            }
        }
        let compression = self.gv(259).and_then(|v| v.first()).filter(|v| *v != 0.0 && !v.is_nan()).unwrap_or(1.0);
        if !(compression == 1.0 || [5.0, 6.0, 7.0, 8.0, 32946.0, 32773.0, 34887.0, 50000.0, 50001.0].contains(&compression)) {
            return Err(format!("Unknown compression method identifier: {}", cc_cad::js::num_to_string(compression)));
        }
        let params = self.decoder_params()?;
        // _readRaster
        let tw = self.tile_width();
        let th = self.tile_height();
        let iw = self.width();
        let ih = self.height();
        let min_xt = crate::math::max((window[0] / tw).floor(), 0.0);
        let max_xt = crate::math::min((window[2] / tw).ceil(), (iw / tw).ceil());
        let min_yt = crate::math::max((window[1] / th).floor(), 0.0);
        let max_yt = crate::math::min((window[3] / th).ceil(), (ih / th).ceil());
        let mut bpp = self.bytes_per_pixel()?;
        let mut src_offsets = Vec::new();
        let mut readers = Vec::new();
        for &s in &samples {
            if self.planar == 1.0 {
                let bits = self.load_value(258)?;
                let Some(TagVal::Arr(b)) = bits else { return Err("Expected BitsPerSample to be an array or typed array.".into()) };
                let sum: f64 = b.v.iter().take(s).sum();
                src_offsets.push(sum / 8.0);
            } else {
                src_offsets.push(0.0);
            }
            readers.push(sample_reader(self.sample_format(s), self.bits_per_sample(s))?);
        }
        let le = self.tiff.le;
        let mut y_t = min_yt;
        let mut tiles_done = 0u64;
        while y_t < max_yt {
            let mut x_t = min_xt;
            while x_t < max_xt {
                tiles_done = tiles_done.saturating_add(1);
                if tiles_done > 50_000_000 {
                    return Err("too many tiles".into());
                }
                let mut tile: Option<Vec<u8>> = None;
                if self.planar == 1.0 {
                    tile = Some(self.tile_or_strip(x_t, y_t, 0.0, compression, &params)?);
                }
                for (si, &sample) in samples.iter().enumerate() {
                    if self.planar == 2.0 {
                        bpp = self.sample_byte_size(sample)?;
                        tile = Some(self.tile_or_strip(x_t, y_t, sample as f64, compression, &params)?);
                    }
                    let Some(buf) = tile.as_ref() else { return Err("Could not get tile or strip data.".into()) };
                    let block_h = self.block_height(y_t);
                    let first_line = y_t * th;
                    let first_col = x_t * tw;
                    let last_line = first_line + block_h;
                    let last_col = (x_t + 1.0) * tw;
                    let reader = readers.get(si).copied().unwrap_or(Reader::U8);
                    let ymax = cc_cad::js::min_of([block_h, block_h - (last_line - window[3]), ih - first_line]);
                    let xmax = cc_cad::js::min_of([tw, tw - (last_col - window[2]), iw - first_col]);
                    let mut y = crate::math::max(0.0, window[1] - first_line);
                    while y < ymax {
                        let mut x = crate::math::max(0.0, window[0] - first_col);
                        while x < xmax {
                            let po = (y * tw + x) * bpp;
                            let value = read_sample(buf, reader, po + src_offsets.get(si).copied().unwrap_or(0.0), le)?;
                            if let Some(a) = inter.as_mut() {
                                let wc = (y + first_line - window[1]) * ww * samples.len() as f64
                                    + (x + first_col - window[0]) * samples.len() as f64
                                    + si as f64;
                                if let Some(i) = idx(wc) {
                                    a.set(i, value);
                                }
                            } else {
                                let wc = (y + first_line - window[1]) * ww + x + first_col - window[0];
                                if let (Some(b), Some(i)) = (bands.get_mut(si), idx(wc)) {
                                    b.set(i, value);
                                }
                            }
                            x += 1.0;
                        }
                        y += 1.0;
                    }
                }
                x_t += 1.0;
            }
            y_t += 1.0;
        }
        let wanted_w = o.width.filter(|v| *v != 0.0 && !v.is_nan());
        let wanted_h = o.height.filter(|v| *v != 0.0 && !v.is_nan());
        if wanted_w.is_some_and(|w| ww != w) || wanted_h.is_some_and(|h| wh != h) {
            let ow = o.width.unwrap_or(ww);
            let oh = o.height.unwrap_or(wh);
            if ow * oh * samples.len().max(1) as f64 > MAX_CELLS || !(ow >= 0.0) || !(oh >= 0.0) {
                return Err("Invalid typed array length".into());
            }
            let method = o.resample.clone().unwrap_or_else(|| "nearest".into()).to_lowercase();
            if method != "nearest" && method != "bilinear" && method != "linear" {
                return Err(format!("Unsupported resampling method: '{}'", o.resample.clone().unwrap_or_default()));
            }
            if let Some(a) = inter {
                let r = resample_interleaved(&a, ww, wh, ow, oh, samples.len(), method != "nearest");
                return Ok(Rasters { kinds: vec![r.kind], bands: vec![r.v], width: o.width.unwrap_or(ww), height: o.height.unwrap_or(wh) });
            }
            let rs: Vec<TArr> = bands.iter().map(|a| resample_one(a, ww, wh, ow, oh, method != "nearest")).collect();
            return Ok(Rasters {
                kinds: rs.iter().map(|r| r.kind).collect(),
                bands: rs.into_iter().map(|r| r.v).collect(),
                width: o.width.unwrap_or(ww),
                height: o.height.unwrap_or(wh),
            });
        }
        let width = wanted_w.unwrap_or(ww);
        let height = wanted_h.unwrap_or(wh);
        if let Some(a) = inter {
            return Ok(Rasters { kinds: vec![a.kind], bands: vec![a.v], width, height });
        }
        Ok(Rasters { kinds: bands.iter().map(|b| b.kind).collect(), bands: bands.into_iter().map(|b| b.v).collect(), width, height })
    }

    /// `readRGB(options)`.
    pub fn read_rgb(&self, o: &ReadOptions) -> R<Rasters> {
        let window = o.window.unwrap_or([0.0, 0.0, self.width(), self.height()]);
        if window[0] > window[2] || window[1] > window[3] {
            return Err("Invalid subsets".into());
        }
        let pi = self.gv(262).and_then(|v| v.first());
        if pi == Some(2.0) {
            let mut s = vec![0usize, 1, 2];
            let extra = self.gv(338);
            if let Some(e) = extra.filter(TagVal::truthy)
                && e.at(0) != Some(0.0)
                && o.enable_alpha
            {
                s = (0..self.bits().len()).collect();
            }
            let mut ro = o.clone();
            ro.samples = s;
            return self.read_rasters(&ro);
        }
        let samples: Vec<usize> = match pi {
            Some(0.0) | Some(1.0) | Some(3.0) => vec![0],
            Some(5.0) => vec![0, 1, 2, 3],
            Some(6.0) | Some(8.0) => vec![0, 1, 2],
            _ => return Err("Invalid or unsupported photometric interpretation.".into()),
        };
        let sub = ReadOptions {
            window: Some(window),
            interleave: true,
            samples,
            width: o.width,
            height: o.height,
            resample: o.resample.clone(),
            ..ReadOptions::default()
        };
        let raster = self.read_rasters(&sub)?;
        let max = 2f64.powf(self.bits_per_sample(0));
        let src = raster.bands.first().cloned().unwrap_or_default();
        let px = to_usize(raster.width * raster.height);
        let mut out = TArr::zeros(if pi == Some(6.0) { Elem::U8C } else { Elem::U8 }, px.saturating_mul(3));
        match pi {
            Some(0.0) => {
                for (i, v) in src.iter().enumerate() {
                    let val = 256.0 - (v / max) * 256.0;
                    out.set(3 * i, val);
                    out.set(3 * i + 1, val);
                    out.set(3 * i + 2, val);
                }
            }
            Some(1.0) => {
                for (i, v) in src.iter().enumerate() {
                    let val = (v / max) * 256.0;
                    out.set(3 * i, val);
                    out.set(3 * i + 1, val);
                    out.set(3 * i + 2, val);
                }
            }
            Some(3.0) => {
                let cm = match self.load_value(320)? {
                    Some(TagVal::Arr(a)) => a.v,
                    _ => Vec::new(),
                };
                let g_off = cm.len() as f64 / 3.0;
                let b_off = (cm.len() as f64 / 3.0) * 2.0;
                let at = |i: f64| idx(i).and_then(|k| cm.get(k).copied()).unwrap_or(f64::NAN);
                for (i, v) in src.iter().enumerate() {
                    out.set(3 * i, (at(*v) / 65536.0) * 256.0);
                    out.set(3 * i + 1, (at(v + g_off) / 65536.0) * 256.0);
                    out.set(3 * i + 2, (at(v + b_off) / 65536.0) * 256.0);
                }
            }
            Some(5.0) => {
                let mut j = 0usize;
                for c in src.chunks(4) {
                    let g = |k: usize| c.get(k).copied().unwrap_or(f64::NAN);
                    let (cc, m, y, k) = (g(0), g(1), g(2), g(3));
                    out.set(j, 255.0 * ((255.0 - cc) / 256.0) * ((255.0 - k) / 256.0));
                    out.set(j + 1, 255.0 * ((255.0 - m) / 256.0) * ((255.0 - k) / 256.0));
                    out.set(j + 2, 255.0 * ((255.0 - y) / 256.0) * ((255.0 - k) / 256.0));
                    j += 3;
                }
            }
            Some(6.0) => {
                let mut j = 0usize;
                for c in src.chunks(3) {
                    let g = |k: usize| c.get(k).copied().unwrap_or(f64::NAN);
                    let (y, cb, cr) = (g(0), g(1), g(2));
                    out.set(j, y + 1.402 * (cr - 128.0));
                    out.set(j + 1, y - 0.34414 * (cb - 128.0) - 0.71414 * (cr - 128.0));
                    out.set(j + 2, y + 1.772 * (cb - 128.0));
                    j += 3;
                }
            }
            _ => {
                let mut j = 0usize;
                for c in src.chunks(3) {
                    let g = |k: usize| c.get(k).copied().unwrap_or(f64::NAN);
                    let l = g(0);
                    let a_ = f64::from(((cc_cad::js::int32(g(1)) as i32) << 24) >> 24);
                    let b_ = f64::from(((cc_cad::js::int32(g(2)) as i32) << 24) >> 24);
                    let mut y = (l + 16.0) / 116.0;
                    let mut x = a_ / 500.0 + y;
                    let mut z = y - b_ / 200.0;
                    x = 0.95047 * (if x * x * x > 0.008856 { x * x * x } else { (x - 16.0 / 116.0) / 7.787 });
                    y = 1.0 * (if y * y * y > 0.008856 { y * y * y } else { (y - 16.0 / 116.0) / 7.787 });
                    z = 1.08883 * (if z * z * z > 0.008856 { z * z * z } else { (z - 16.0 / 116.0) / 7.787 });
                    let mut r = x * 3.2406 + y * -1.5372 + z * -0.4986;
                    let mut g2 = x * -0.9689 + y * 1.8758 + z * 0.0415;
                    let mut b = x * 0.0557 + y * -0.204 + z * 1.057;
                    r = if r > 0.0031308 { 1.055 * crate::math::pow(r, 1.0 / 2.4) - 0.055 } else { 12.92 * r };
                    g2 = if g2 > 0.0031308 { 1.055 * crate::math::pow(g2, 1.0 / 2.4) - 0.055 } else { 12.92 * g2 };
                    b = if b > 0.0031308 { 1.055 * crate::math::pow(b, 1.0 / 2.4) - 0.055 } else { 12.92 * b };
                    out.set(j, crate::math::max(0.0, crate::math::min(1.0, r)) * 255.0);
                    out.set(j + 1, crate::math::max(0.0, crate::math::min(1.0, g2)) * 255.0);
                    out.set(j + 2, crate::math::max(0.0, crate::math::min(1.0, b)) * 255.0);
                    j += 3;
                }
            }
        }
        if !o.interleave {
            let n3 = out.len() / 3;
            let mut r = Vec::with_capacity(n3);
            let mut g = Vec::with_capacity(n3);
            let mut b = Vec::with_capacity(n3);
            for c in out.v.chunks(3) {
                r.push(c.first().copied().unwrap_or(0.0));
                g.push(c.get(1).copied().unwrap_or(0.0));
                b.push(c.get(2).copied().unwrap_or(0.0));
            }
            return Ok(Rasters { bands: vec![r, g, b], kinds: vec![Elem::U8; 3], width: raster.width, height: raster.height });
        }
        Ok(Rasters { kinds: vec![out.kind], bands: vec![out.v], width: raster.width, height: raster.height })
    }
}

fn js_substring(s: &str, a: f64, b: f64) -> String {
    let u: Vec<u16> = s.encode_utf16().collect();
    let len = u.len() as f64;
    let fix = |v: f64| if v.is_nan() { 0.0 } else { v.max(0.0).min(len) };
    let (mut x, mut y) = (fix(a), fix(b));
    if x > y {
        std::mem::swap(&mut x, &mut y);
    }
    String::from_utf16_lossy(u.get(to_usize(x)..to_usize(y)).unwrap_or(&[]))
}
fn js_subarray(v: &[f64], a: f64, b: f64) -> Vec<f64> {
    let len = v.len() as f64;
    let fix = |x: f64| {
        let t = if x.is_nan() { 0.0 } else { x.trunc() };
        if t < 0.0 { (len + t).max(0.0) } else { t.min(len) }
    };
    let (x, y) = (fix(a), fix(b));
    if y <= x {
        return Vec::new();
    }
    v.get(to_usize(x)..to_usize(y)).map(<[f64]>::to_vec).unwrap_or_default()
}

#[derive(Clone, Copy, Debug)]
enum Reader {
    U8,
    U16,
    U32,
    I8,
    I16,
    I32,
    F16,
    F32,
    F64,
}

fn sample_reader(format: f64, bits: f64) -> R<Reader> {
    let r = if format == 1.0 {
        if bits <= 8.0 {
            Some(Reader::U8)
        } else if bits <= 16.0 {
            Some(Reader::U16)
        } else if bits <= 32.0 {
            Some(Reader::U32)
        } else {
            None
        }
    } else if format == 2.0 {
        if bits <= 8.0 {
            Some(Reader::I8)
        } else if bits <= 16.0 {
            Some(Reader::I16)
        } else if bits <= 32.0 {
            Some(Reader::I32)
        } else {
            None
        }
    } else if format == 3.0 {
        if bits == 16.0 {
            Some(Reader::F16)
        } else if bits == 32.0 {
            Some(Reader::F32)
        } else if bits == 64.0 {
            Some(Reader::F64)
        } else {
            None
        }
    } else {
        None
    };
    r.ok_or_else(|| "Unsupported data format/bitsPerSample".to_string())
}

fn f16_to_f64(h: u16) -> f64 {
    let s = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = (h >> 10) & 0x1f;
    let m = f64::from(h & 0x3ff);
    if e == 0 {
        s * m * 2f64.powi(-24)
    } else if e == 31 {
        if m == 0.0 { s * f64::INFINITY } else { f64::NAN }
    } else {
        s * (1.0 + m / 1024.0) * 2f64.powi(i32::from(e) - 15)
    }
}

fn read_sample(buf: &[u8], r: Reader, off: f64, le: bool) -> R<f64> {
    let b = Bytes { b: buf, le };
    match r {
        Reader::U8 => b.u8(off),
        Reader::I8 => b.i8(off),
        Reader::U16 => b.u16(off),
        Reader::I16 => b.i16(off),
        Reader::U32 => b.u32(off),
        Reader::I32 => b.i32(off),
        Reader::F32 => b.f32(off),
        Reader::F64 => b.f64(off),
        Reader::F16 => {
            let a = b.get::<2>(off)?;
            // float16's getFloat16 rounds through Float32 (its own conversion): exact for every half value
            Ok(f16_to_f64(if le { u16::from_le_bytes(a) } else { u16::from_be_bytes(a) }))
        }
    }
}

/// Write an element in the platform's (little-endian) layout, as a typed array over an ArrayBuffer does.
fn write_native(out: &mut [u8], kind: Elem, i: usize, v: f64) {
    let at = i.saturating_mul(kind.size());
    let mut put = |bytes: &[u8]| {
        if let Some(s) = out.get_mut(at..at.saturating_add(bytes.len())) {
            s.copy_from_slice(bytes);
        }
    };
    match kind {
        Elem::U8 | Elem::U8C => put(&[v as u8]),
        Elem::I8 => put(&(v as i8).to_le_bytes()),
        Elem::U16 => put(&(v as u16).to_le_bytes()),
        Elem::I16 => put(&(v as i16).to_le_bytes()),
        Elem::U32 => put(&(v as u32).to_le_bytes()),
        Elem::I32 => put(&(v as i32).to_le_bytes()),
        Elem::F32 => put(&(v as f32).to_le_bytes()),
        Elem::F64 | Elem::Js => put(&v.to_le_bytes()),
    }
}

#[derive(Clone, Debug)]
struct DecoderParams {
    tile_width: f64,
    tile_height: f64,
    planar: Option<f64>,
    bits: Vec<f64>,
    predictor: f64,
}

fn needs_normalization(format: f64, bits: f64) -> bool {
    if (format == 1.0 || format == 2.0) && bits <= 32.0 && bits % 8.0 == 0.0 {
        return false;
    }
    if format == 3.0 && (bits == 16.0 || bits == 32.0 || bits == 64.0) {
        return false;
    }
    true
}

fn normalize_array(inb: &[u8], format: f64, planar: f64, spp: f64, bits: f64, tw: f64, th: f64) -> R<Vec<u8>> {
    let out_size = if planar == 2.0 { th * tw } else { th * tw * spp };
    let to_transfer = if planar == 2.0 { 1.0 } else { spp };
    let kind = Image::array_kind(format, bits)?;
    if out_size > MAX_CELLS {
        return Err("Array buffer allocation failed".into());
    }
    let mut out = TArr::zeros(kind, to_usize(out_size));
    let mask = if (1.0..=53.0).contains(&bits) { 2f64.powf(bits) - 1.0 } else { f64::NAN };
    let view = Bytes { b: inb, le: false };
    if format == 1.0 {
        let pixel_skip = if planar == 1.0 { spp * bits } else { bits };
        let mut bits_per_line = tw * pixel_skip;
        if (bits_per_line as i64) & 7 != 0 {
            bits_per_line = f64::from(((cc_cad::js::int32(bits_per_line) as i32).wrapping_add(7)) & !7);
        }
        let mut y = 0.0;
        while y < th {
            let line = y * bits_per_line;
            let mut x = 0.0;
            while x < tw {
                let pix = line + x * to_transfer * bits;
                let mut i = 0.0;
                while i < to_transfer {
                    let bit = pix + i * bits;
                    let oi = ((y * tw) + x) * to_transfer + i;
                    let byte = (bit / 8.0).floor();
                    let inner = bit % 8.0;
                    let shift = |total: f64| -> i32 { cc_cad::js::int32(total - bits - inner) as i32 };
                    let m = cc_cad::js::int32(mask) as i32;
                    let v: i32 = if inner + bits <= 8.0 {
                        (view.u8(byte)? as i32).wrapping_shr(shift(8.0) as u32 & 31) & m
                    } else if inner + bits <= 16.0 {
                        (view.u16(byte)? as i32).wrapping_shr(shift(16.0) as u32 & 31) & m
                    } else if inner + bits <= 24.0 {
                        let raw = ((view.u16(byte)? as i32) << 8) | (view.u8(byte + 1.0)? as i32);
                        raw.wrapping_shr(shift(24.0) as u32 & 31) & m
                    } else {
                        let u = view.u32(byte)?;
                        (cc_cad::js::int32(u) as i32).wrapping_shr(shift(32.0) as u32 & 31) & m
                    };
                    if let Some(k) = idx(oi) {
                        out.set(k, f64::from(v));
                    }
                    i += 1.0;
                }
                x += 1.0;
            }
            y += 1.0;
        }
    }
    let mut bytes = vec![0u8; out.len().saturating_mul(kind.size())];
    for (i, v) in out.v.iter().enumerate() {
        write_native(&mut bytes, kind, i, *v);
    }
    Ok(bytes)
}

fn apply_predictor(mut block: Vec<u8>, p: &DecoderParams) -> R<Vec<u8>> {
    let predictor = p.predictor;
    if predictor == 0.0 || predictor.is_nan() || predictor == 1.0 {
        return Ok(block);
    }
    for b in &p.bits {
        if b % 8.0 != 0.0 {
            return Err("When decoding with predictor, only multiple of 8 bits are supported.".into());
        }
        if *b != p.bits.first().copied().unwrap_or(f64::NAN) {
            return Err("When decoding with predictor, all samples must have the same size.".into());
        }
    }
    let b0 = p.bits.first().copied().unwrap_or(f64::NAN);
    let bps = b0 / 8.0;
    let stride = if p.planar == Some(2.0) { 1.0 } else { p.bits.len() as f64 };
    let width = p.tile_width;
    let len = block.len() as f64;
    let mut i = 0.0;
    while i < p.tile_height {
        let start = i * stride * width * bps;
        if start >= len {
            break;
        }
        let row_bytes = stride * width * bps;
        if predictor == 2.0 {
            let elem = match b0 as i64 {
                8 => 1usize,
                16 => 2,
                32 => 4,
                _ => return Err(format!("Predictor 2 not allowed with {} bits per sample.", cc_cad::js::num_to_string(b0))),
            };
            let s = to_usize(start);
            let n = to_usize(row_bytes / elem as f64);
            if s.saturating_add(n.saturating_mul(elem)) > block.len() {
                return Err(format!("Invalid typed array length: {n}"));
            }
            let st = to_usize(stride);
            // decodeRowAcc on a platform (little-endian) typed array view
            let get = |b: &[u8], k: usize| -> u64 {
                let o = s + k * elem;
                match elem {
                    1 => u64::from(b.get(o).copied().unwrap_or(0)),
                    2 => u64::from(u16::from_le_bytes([b.get(o).copied().unwrap_or(0), b.get(o + 1).copied().unwrap_or(0)])),
                    _ => u64::from(u32::from_le_bytes([
                        b.get(o).copied().unwrap_or(0),
                        b.get(o + 1).copied().unwrap_or(0),
                        b.get(o + 2).copied().unwrap_or(0),
                        b.get(o + 3).copied().unwrap_or(0),
                    ])),
                }
            };
            let mut length = n as i64 - st as i64;
            let mut offset = 0usize;
            loop {
                for _ in 0..st {
                    let k = offset + st;
                    if k < n {
                        let v = get(&block, k).wrapping_add(get(&block, offset));
                        let o = s + k * elem;
                        match elem {
                            1 => {
                                if let Some(x) = block.get_mut(o) {
                                    *x = v as u8;
                                }
                            }
                            2 => {
                                if let Some(x) = block.get_mut(o..o + 2) {
                                    x.copy_from_slice(&(v as u16).to_le_bytes());
                                }
                            }
                            _ => {
                                if let Some(x) = block.get_mut(o..o + 4) {
                                    x.copy_from_slice(&(v as u32).to_le_bytes());
                                }
                            }
                        }
                    }
                    offset += 1;
                }
                length -= st as i64;
                if !(length > 0) || st == 0 {
                    break;
                }
            }
        } else if predictor == 3.0 {
            let s = to_usize(start);
            let n = to_usize(row_bytes);
            if s.saturating_add(n) > block.len() {
                return Err(format!("Invalid typed array length: {n}"));
            }
            let st = to_usize(stride);
            let bpsu = to_usize(bps);
            let row = block.get_mut(s..s + n).ok_or(OOB)?;
            let wc = n.checked_div(bpsu).unwrap_or(0);
            let mut index = 0usize;
            let mut count = n;
            while count > st && st > 0 {
                for _ in 0..st {
                    let a = row.get(index).copied().unwrap_or(0);
                    if let Some(x) = row.get_mut(index + st) {
                        *x = x.wrapping_add(a);
                    }
                    index += 1;
                }
                count -= st;
            }
            let copy = row.to_vec();
            for i2 in 0..wc {
                for b in 0..bpsu {
                    let src = copy.get((bpsu - b - 1) * wc + i2).copied().unwrap_or(0);
                    if let Some(x) = row.get_mut(bpsu * i2 + b) {
                        *x = src;
                    }
                }
            }
        }
        i += 1.0;
    }
    Ok(block)
}

fn lerp(v0: f64, v1: f64, t: f64) -> f64 {
    (1.0 - t) * v0 + t * v1
}

fn resample_one(a: &TArr, iw: f64, ih: f64, ow: f64, oh: f64, bilinear: bool) -> TArr {
    let rel_x = iw / ow;
    let rel_y = ih / oh;
    let mut out = TArr::zeros(a.kind, to_usize(ow * oh));
    let at = |i: f64| idx(i).and_then(|k| a.v.get(k).copied()).unwrap_or(f64::NAN);
    let mut y = 0.0;
    while y < oh {
        if bilinear {
            let raw_y = rel_y * y;
            let yl = raw_y.floor();
            let yh = crate::math::min(raw_y.ceil(), ih - 1.0);
            let mut x = 0.0;
            while x < ow {
                let raw_x = rel_x * x;
                let tx = raw_x % 1.0;
                let xl = raw_x.floor();
                let xh = crate::math::min(raw_x.ceil(), iw - 1.0);
                let ll = at(yl * iw + xl);
                let hl = at(yl * iw + xh);
                let lh = at(yh * iw + xl);
                let hh = at(yh * iw + xh);
                let v = lerp(lerp(ll, hl, tx), lerp(lh, hh, tx), raw_y % 1.0);
                if let Some(k) = idx(y * ow + x) {
                    out.set(k, v);
                }
                x += 1.0;
            }
        } else {
            let cy = crate::math::min(crate::math::round(rel_y * y), ih - 1.0);
            let mut x = 0.0;
            while x < ow {
                let cx = crate::math::min(crate::math::round(rel_x * x), iw - 1.0);
                if let Some(k) = idx(y * ow + x) {
                    out.set(k, at(cy * iw + cx));
                }
                x += 1.0;
            }
        }
        y += 1.0;
    }
    out
}

fn resample_interleaved(a: &TArr, iw: f64, ih: f64, ow: f64, oh: f64, samples: usize, bilinear: bool) -> TArr {
    let rel_x = iw / ow;
    let rel_y = ih / oh;
    let s = samples as f64;
    let mut out = TArr::zeros(a.kind, to_usize(ow * oh * s));
    let at = |i: f64| idx(i).and_then(|k| a.v.get(k).copied()).unwrap_or(f64::NAN);
    let mut y = 0.0;
    while y < oh {
        if bilinear {
            let raw_y = rel_y * y;
            let yl = raw_y.floor();
            let yh = crate::math::min(raw_y.ceil(), ih - 1.0);
            let mut x = 0.0;
            while x < ow {
                let raw_x = rel_x * x;
                let tx = raw_x % 1.0;
                let xl = raw_x.floor();
                let xh = crate::math::min(raw_x.ceil(), iw - 1.0);
                for i in 0..samples {
                    let fi = i as f64;
                    let ll = at(yl * iw * s + xl * s + fi);
                    let hl = at(yl * iw * s + xh * s + fi);
                    let lh = at(yh * iw * s + xl * s + fi);
                    let hh = at(yh * iw * s + xh * s + fi);
                    let v = lerp(lerp(ll, hl, tx), lerp(lh, hh, tx), raw_y % 1.0);
                    if let Some(k) = idx(y * ow * s + x * s + fi) {
                        out.set(k, v);
                    }
                }
                x += 1.0;
            }
        } else {
            let cy = crate::math::min(crate::math::round(rel_y * y), ih - 1.0);
            let mut x = 0.0;
            while x < ow {
                let cx = crate::math::min(crate::math::round(rel_x * x), iw - 1.0);
                for i in 0..samples {
                    let fi = i as f64;
                    if let Some(k) = idx(y * ow * s + x * s + fi) {
                        out.set(k, at(cy * iw * s + cx * s + fi));
                    }
                }
                x += 1.0;
            }
        }
        y += 1.0;
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------
// decoders

/// geotiff.js's LZW (its own bit reader and dictionary, quirks kept). Bounded where the JS would loop forever.
pub fn lzw_decompress(input: &[u8]) -> R<Vec<u8>> {
    const CLEAR: u32 = 256;
    const EOI: u32 = 257;
    let mut dict_index = vec![0u16; 4093];
    let mut dict_char = vec![0u8; 4093];
    for i in 0..=257usize {
        if let Some(x) = dict_index.get_mut(i) {
            *x = 4096;
        }
        if let Some(x) = dict_char.get_mut(i) {
            *x = i as u8;
        }
    }
    let mut dict_len: u32 = 258;
    let mut byte_len: u32 = 9;
    let mut position: u64 = 0;
    let get_byte = |position: u64, length: u32| -> u32 {
        let d = (position % 8) as i64;
        let a = (position / 8) as i64;
        let de = 8 - d;
        let len = i64::from(length);
        let ef = (position as i64 + len) - ((a + 1) * 8);
        let mut fg = (8 * (a + 2)) - (position as i64 + len);
        let dg = ((a + 2) * 8) - position as i64;
        fg = fg.max(0);
        let n = input.len() as i64;
        if a >= n {
            return EOI;
        }
        let byte = |k: i64| i64::from(input.get(usize::try_from(k).unwrap_or(usize::MAX)).copied().unwrap_or(0));
        let mut chunk1 = byte(a) & ((1i64 << (8 - d)) - 1);
        chunk1 = i64::from((chunk1 as i32).wrapping_shl((len - de) as u32 & 31));
        let mut chunks = chunk1;
        if a + 1 < n {
            let mut chunk2 = byte(a + 1) >> (fg.min(31));
            if fg > 31 {
                chunk2 = 0;
            }
            chunk2 = i64::from((chunk2 as i32).wrapping_shl((len - dg).max(0) as u32 & 31));
            chunks += chunk2;
        }
        if ef > 8 && a + 2 < n {
            let hi = ((a + 3) * 8) - (position as i64 + len);
            let chunk3 = if (0..32).contains(&hi) { byte(a + 2) >> hi } else { byte(a + 2) >> (hi & 31) };
            chunks += chunk3;
        }
        chunks as u32
    };
    let rev = |dict_index: &[u16], dict_char: &[u8], n: u32| -> R<Vec<u8>> {
        let mut out = Vec::new();
        let mut i = n;
        let mut guard = 0u32;
        while i != 4096 {
            let iu = i as usize;
            let (Some(&c), Some(&nx)) = (dict_char.get(iu), dict_index.get(iu)) else {
                return Err("LZW dictionary index out of range".into());
            };
            out.push(c);
            i = u32::from(nx);
            guard += 1;
            if guard > 4096 {
                return Err("LZW dictionary loop".into());
            }
        }
        Ok(out)
    };
    let mut result: Vec<u8> = Vec::new();
    let next = |position: &mut u64, byte_len: u32| -> u32 {
        let b = get_byte(*position, byte_len);
        *position += u64::from(byte_len);
        b
    };
    let mut code = next(&mut position, byte_len);
    let mut old: Option<u32> = None;
    let add = |dict_index: &mut Vec<u16>, dict_char: &mut Vec<u8>, dict_len: &mut u32, i: u32, c: u8| {
        if let Some(x) = dict_char.get_mut(*dict_len as usize) {
            *x = c;
        }
        if let Some(x) = dict_index.get_mut(*dict_len as usize) {
            *x = i as u16;
        }
        *dict_len += 1;
    };
    let mut steps = 0u64;
    while code != EOI {
        steps += 1;
        if steps > 100_000_000 || result.len() > 1_000_000_000 {
            return Err("LZW stream too long".into());
        }
        if code == CLEAR {
            dict_len = 258;
            byte_len = 9;
            code = next(&mut position, byte_len);
            while code == CLEAR {
                code = next(&mut position, byte_len);
            }
            if code == EOI {
                break;
            } else if code > CLEAR {
                return Err(format!("corrupted code at scanline {code}"));
            } else {
                let val = rev(&dict_index, &dict_char, code)?;
                result.extend(val.iter().rev());
                old = Some(code);
            }
        } else if code < dict_len {
            let val = rev(&dict_index, &dict_char, code)?;
            result.extend(val.iter().rev());
            if let Some(o) = old {
                add(&mut dict_index, &mut dict_char, &mut dict_len, o, val.last().copied().unwrap_or(0));
            }
            old = Some(code);
        } else {
            let Some(o) = old else { return Err(format!("Invalid LZW code: {code} with no previous code")) };
            let ov = rev(&dict_index, &dict_char, o)?;
            result.extend(ov.iter().rev());
            let last = ov.last().copied().unwrap_or(0);
            result.push(last);
            add(&mut dict_index, &mut dict_char, &mut dict_len, o, last);
            old = Some(code);
        }
        if dict_len + 1 >= (1u32 << byte_len) {
            if byte_len == 12 {
                old = None;
            } else {
                byte_len += 1;
            }
        }
        code = next(&mut position, byte_len);
    }
    Ok(result)
}

/// pako.inflate (zlib stream).
pub fn inflate_zlib(raw: &[u8]) -> R<Vec<u8>> {
    // pako's messages for what can be told from the stream's header and first block; other damage inside the
    // compressed data gets one generic pako message (miniz_oxide does not say which check failed)
    if let (Some(&cmf), Some(&flg)) = (raw.first(), raw.get(1)) {
        if (u32::from(cmf) * 256 + u32::from(flg)) % 31 != 0 {
            return Err("incorrect header check".into());
        }
        if cmf & 0x0f != 8 {
            return Err("unknown compression method".into());
        }
        if (cmf >> 4) + 8 > 15 {
            return Err("invalid window size".into());
        }
        if flg & 0x20 != 0 {
            return Err("need dictionary".into());
        }
        if let Some(&b0) = raw.get(2)
            && (b0 >> 1) & 3 == 3
        {
            return Err("invalid block type".into());
        }
    }
    miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(raw, 1usize << 31).map_err(|e| match e.status {
        miniz_oxide::inflate::TINFLStatus::FailedCannotMakeProgress | miniz_oxide::inflate::TINFLStatus::NeedsMoreInput => {
            "buffer error".to_string()
        }
        miniz_oxide::inflate::TINFLStatus::Adler32Mismatch => "incorrect data check".to_string(),
        _ => "invalid stored block lengths".to_string(),
    })
}

/// geotiff.js's PackBits (−128 is a run of 129, as it reads it).
pub fn packbits(buf: &[u8]) -> R<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < buf.len() {
        let header = i8::from_le_bytes([buf.get(i).copied().ok_or(OOB)?]);
        if header < 0 {
            let next = *buf.get(i + 1).ok_or(OOB)?;
            let h = -i32::from(header);
            for _ in 0..=h {
                out.push(next);
            }
            i += 1;
        } else {
            for j in 0..=usize::try_from(header).unwrap_or(0) {
                out.push(*buf.get(i + j + 1).ok_or(OOB)?);
            }
            i += usize::try_from(header).unwrap_or(0) + 1;
        }
        i += 1;
        if out.len() > 1_000_000_000 {
            return Err("PackBits output too long".into());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn elem_conv() {
        assert_eq!(Elem::U8.conv(257.9), 1.0);
        assert_eq!(Elem::I8.conv(200.0), -56.0);
        assert_eq!(Elem::U8C.conv(2.5), 2.0);
        assert_eq!(Elem::U8C.conv(3.5), 4.0);
        assert_eq!(Elem::U8C.conv(-3.0), 0.0);
    }
    #[test]
    fn bad_header() {
        assert!(Tiff::from_bytes(b"XX").is_err());
        assert!(Tiff::from_bytes(b"II\x2a\x00\x08\x00\x00\x00").is_ok());
    }
    /// A tiny RGB TIFF claiming 12 000 × 11 000 pixels: the read is refused, not a 3.2 GB allocation (fuzz OOM 2026-10-21).
    #[test]
    fn claimed_size_capped() {
        let tags: [(u16, u16, u32); 9] = [
            (256, 4, 12_000),
            (257, 4, 11_000),
            (258, 3, 8),
            (259, 3, 1),
            (262, 3, 2),
            (273, 4, 8),
            (277, 3, 3),
            (278, 4, 11_000),
            (279, 4, 1),
        ];
        let mut b = b"II\x2a\x00\x0a\x00\x00\x00\x00\x00".to_vec();
        b.extend_from_slice(&(tags.len() as u16).to_le_bytes());
        for (tag, ty, v) in tags {
            b.extend_from_slice(&tag.to_le_bytes());
            b.extend_from_slice(&ty.to_le_bytes());
            b.extend_from_slice(&1u32.to_le_bytes());
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&0u32.to_le_bytes());
        let tiff = Tiff::from_bytes(&b).expect("header");
        let img = tiff.image(0).expect("image");
        assert_eq!(img.read_rgb(&ReadOptions::default()).err().as_deref(), Some("Invalid typed array length"));
        assert_eq!(img.read_rasters(&ReadOptions::default()).err().as_deref(), Some("Invalid typed array length"));
    }
}
