//! pointcloud.js — point clouds for CadCraft (and later BimCraft's IFC view): LAS / LAZ through [`crate::las`] (the
//! header, the coordinate system and the LAZ decoder are CivilCraft's: never a second LAS reader), XYZ / PTS text read
//! here. Every point is read once; at most `budget` are kept, evenly through the file. Positions are f32 offsets from
//! `origin` (the low corner of what was read, whole units), so survey coordinates keep their millimetres.
//!
//! Files are hostile: sizes are checked before anything is read (2000 MB, 400 MB of text, 1024 MB of LAZ), the kept
//! points are capped by the budget (at most 20 million), and every count, offset and index is checked.
//!
//! The numbers are the JS's: the keep rate accumulates in the same order, colours by the same ramp and rounding, the
//! slice and the snap grid by the same comparisons (`bin/test-pointcloud.mjs` and kdiff-cadcloud.mjs check them).

// Every cast below is of a value already rounded and clamped into the target's range (colours 0–255, intensities
// 0–65535, grid keys under 2^53), or a double narrowed to the f32 the JS keeps (`Float32Array`).
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use std::collections::HashMap;

use cc_cad::i18n::Catalog;
use cc_civil::js::{num_to_string, round, usize_of};

use crate::las::{Header, class_name, read_las_header_lenient, scan_records};
use crate::raster::{InFile, locale_int};

type R<T> = Result<T, String>;

/// `UNIT_M`: metres per unit of a cloud's coordinates.
pub fn unit_m(u: &str) -> Option<f64> {
    match u {
        "m" => Some(1.0),
        "ft" => Some(0.3048),
        "ftUS" => Some(1200.0 / 3937.0),
        _ => None,
    }
}
pub const CLOUD_EXT: [&str; 4] = ["las", "laz", "xyz", "pts"];
/// `LIMITS`
pub const FILE_BYTES: f64 = 2000.0 * 1024.0 * 1024.0;
pub const TEXT_BYTES: f64 = 400.0 * 1024.0 * 1024.0;
pub const BUDGET_MAX: f64 = 20e6;
/// LAZ files are held whole while they are read
pub const LAZ_BYTES: f64 = 1024.0 * 1048576.0;

/// `COLOR_AT`: where a point format keeps its colour.
fn color_at(fmt: f64) -> Option<usize> {
    match fmt as i64 {
        2 => Some(20),
        3 | 5 => Some(28),
        7 | 8 | 10 => Some(30),
        _ => None,
    }
}

/// What kind of cloud a file is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Las,
    Laz,
    Xyz,
}
impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Las => "las",
            Kind::Laz => "laz",
            Kind::Xyz => "xyz",
        }
    }
}

/// `sniffCloud(name, head)` → "las" | "xyz" | None: the LASF signature, else the extension.
pub fn sniff(name: &str, head: &[u8]) -> Option<&'static str> {
    if head.starts_with(b"LASF") {
        return Some("las");
    }
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "las" | "laz" => Some("las"),
        "xyz" | "pts" => Some("xyz"),
        _ => None,
    }
}

/// Which column of a text cloud is what (`textColumns(n)`): intensity and the first colour column, if any.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Columns {
    pub n: usize,
    pub i: Option<usize>,
    pub rgb: Option<usize>,
}
pub fn text_columns(n: usize) -> Columns {
    if n >= 7 {
        Columns { n, i: Some(3), rgb: Some(4) }
    } else if n == 6 {
        Columns { n, i: None, rgb: Some(3) }
    } else if n >= 4 {
        Columns { n, i: Some(3), rgb: None }
    } else {
        Columns { n, i: None, rgb: None }
    }
}

/// `readCloudInfo(file)`: the header (LAS / LAZ) or the first 64 KB (text).
#[derive(Clone, Debug)]
pub struct Info {
    pub kind: Kind,
    pub name: String,
    pub size: f64,
    /// exact for LAS / LAZ, an estimate for text
    pub count: f64,
    pub estimate: bool,
    pub color: bool,
    pub intensity: bool,
    pub classes: bool,
    /// x and y: "m" | "ft" | "ftUS"
    pub unit: Option<String>,
    pub z_unit: Option<String>,
    pub notes: Vec<String>,
    pub las: Option<Header>,
    pub columns: Option<Columns>,
}

fn js_err(tr: &Catalog, key: &str, vars: &[(&str, String)]) -> String {
    tr.t(key, vars)
}
fn mb(size: f64) -> String {
    num_to_string(round(size / 1048576.0))
}

/// `readCloudInfo(file)`.
pub fn read_cloud_info(tr: &Catalog, file: &InFile) -> R<Info> {
    let head = file.bytes.get(..4).unwrap_or(&file.bytes);
    match sniff(&file.name, head) {
        Some("las") => las_info(tr, file),
        Some(_) => text_info(tr, file),
        None => Err(js_err(tr, "pc.not_cloud", &[("name", file.name.clone())])),
    }
}

fn las_info(tr: &Catalog, file: &InFile) -> R<Info> {
    // the coordinate system is matched against crs.js's catalog: loaded once (built in here; the page fetches it)
    crate::crs::set_catalog();
    let size = file.bytes.len() as f64;
    if size > FILE_BYTES {
        return Err(js_err(tr, "pc.too_big", &[("name", file.name.clone()), ("mb", mb(size)), ("max", "2000".into())]));
    }
    let h = read_las_header_lenient(file)?;
    if h.compressed && size > LAZ_BYTES {
        return Err(js_err(tr, "pc.laz_too_big", &[("name", file.name.clone()), ("mb", mb(size))]));
    }
    let crs_unit = h.crs.get("unit").and_then(serde_json::Value::as_str).map(str::to_string);
    let xy_unit = crs_unit.clone().filter(|u| !u.is_empty() && u != "deg");
    let mut notes = h.notes.clone();
    if crs_unit.as_deref() == Some("deg") {
        notes.push(tr.s("pc.degrees"));
    }
    let z_unit = h.z_unit.map(str::to_string).or_else(|| xy_unit.clone());
    Ok(Info {
        kind: if h.compressed { Kind::Laz } else { Kind::Las },
        name: file.name.clone(),
        size,
        count: if h.compressed { h.count } else { h.available },
        estimate: false,
        color: color_at(h.format).is_some(),
        intensity: true,
        classes: true,
        unit: xy_unit,
        z_unit,
        notes,
        las: Some(h),
        columns: None,
    })
}

/// `numbers(line)`: the line's numbers (split at whitespace, commas, semicolons), or None when one is not a number.
fn numbers(line: &str) -> Option<Vec<f64>> {
    let mut out = Vec::new();
    numbers_into(line, &mut out).then_some(out)
}

/// A separator of `/[\s,;]+/` (JS whitespace, comma, semicolon).
fn is_sep(c: char) -> bool {
    match c {
        ' ' | '\t' | ',' | ';' | '\n' | '\r' | '\u{b}' | '\u{c}' => true,
        c if c.is_ascii() => false,
        c => cc_cad::js::is_ws(c),
    }
}

/// [`numbers`] into `out` (cleared first) → false when a part is not a finite number.
fn numbers_into(line: &str, out: &mut Vec<f64>) -> bool {
    out.clear();
    for p in line.split(is_sep) {
        if p.is_empty() {
            continue;
        }
        // Rust's float syntax is JS's decimal one (besides inf / nan words, which are not finite either way); the
        // rest (0x, 0o, 0b, Infinity) goes the JS way
        let v = match p.parse::<f64>() {
            Ok(v) => v,
            Err(_) => cc_cad::js::str_to_number(p),
        };
        if !v.is_finite() {
            return false;
        }
        out.push(v);
    }
    true
}

/// `text.split(/\r?\n/)`.
fn split_lines(text: &str) -> Vec<&str> {
    text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect()
}

/// `new TextDecoder().decode(bytes)`: UTF-8 with replacement characters, a leading BOM dropped.
fn decode_utf8(b: &[u8]) -> String {
    let s = String::from_utf8_lossy(b);
    match s.strip_prefix('\u{feff}') {
        Some(r) => r.to_string(),
        None => s.into_owned(),
    }
}

fn text_info(tr: &Catalog, file: &InFile) -> R<Info> {
    let size = file.bytes.len() as f64;
    if size > TEXT_BYTES {
        return Err(js_err(tr, "pc.too_big", &[("name", file.name.clone()), ("mb", mb(size)), ("max", "400".into())]));
    }
    let text = decode_utf8(file.bytes.get(..65536).unwrap_or(&file.bytes));
    let mut lines = split_lines(&text);
    if size > 65536.0 {
        lines.pop(); // the last line may be cut
    }
    let mut first = 0usize;
    let mut declared: Option<f64> = None;
    if let Some(one) = numbers(lines.first().copied().unwrap_or(""))
        && let [v] = one.as_slice()
        && v.fract() == 0.0
    {
        declared = Some(*v); // PTS count line
        first = 1;
    }
    let (mut rows, mut bytes) = (0usize, 0.0f64);
    let mut counts: Vec<(usize, usize)> = Vec::new();
    for l in lines.iter().skip(first) {
        let Some(v) = numbers(l) else { continue };
        if v.len() < 3 {
            continue;
        }
        match counts.iter_mut().find(|(n, _)| *n == v.len()) {
            Some(c) => c.1 = c.1.saturating_add(1),
            None => counts.push((v.len(), 1)),
        }
        rows = rows.saturating_add(1);
        bytes += l.encode_utf16().count() as f64 + 1.0;
    }
    if rows == 0 {
        return Err(js_err(tr, "pc.text.none", &[("name", file.name.clone())]));
    }
    // the most common column count (the first seen of equals: a stable sort)
    let mut cols = 0usize;
    let mut best = 0usize;
    for (n, c) in &counts {
        if *c > best {
            best = *c;
            cols = *n;
        }
    }
    let columns = text_columns(cols);
    let rows_f = rows as f64;
    let count = declared.unwrap_or_else(|| rows_f.max(round(size / (bytes / rows_f).max(8.0))));
    Ok(Info {
        kind: Kind::Xyz,
        name: file.name.clone(),
        size,
        count,
        estimate: declared.is_none(),
        color: columns.rgb.is_some(),
        intensity: columns.i.is_some(),
        classes: false,
        unit: None,
        z_unit: None,
        notes: Vec::new(),
        las: None,
        columns: Some(columns),
    })
}

// --------------------------------------------------------------------- loading

/// Keeps at most `cap` points evenly: when full, every other kept point goes and the keep rate halves.
struct Keeper {
    cap: usize,
    rate: f64,
    acc: f64,
    n: usize,
    x: Vec<f64>,
    y: Vec<f64>,
    z: Vec<f64>,
    rgb: Option<Vec<u16>>,
    inten: Option<Vec<u16>>,
    cls: Option<Vec<u8>>,
}

impl Keeper {
    fn new(cap: f64, rate: f64, rgb: bool, inten: bool, cls: bool) -> Keeper {
        let cap = usize_of(cap.floor().max(1.0)).unwrap_or(1).max(1);
        Keeper {
            cap,
            rate: rate.min(1.0),
            acc: 0.0,
            n: 0,
            x: vec![0.0; cap],
            y: vec![0.0; cap],
            z: vec![0.0; cap],
            rgb: rgb.then(|| vec![0; cap.saturating_mul(3)]),
            inten: inten.then(|| vec![0; cap]),
            cls: cls.then(|| vec![0; cap]),
        }
    }
    fn want(&mut self) -> bool {
        self.acc += self.rate;
        if self.acc < 1.0 {
            return false;
        }
        self.acc -= 1.0;
        if self.n == self.cap {
            self.halve();
        }
        true
    }
    fn halve(&mut self) {
        let mut k = 0usize;
        let mut j = 0usize;
        while j < self.n {
            for v in [&mut self.x, &mut self.y, &mut self.z] {
                if let Some(a) = v.get(j).copied()
                    && let Some(s) = v.get_mut(k)
                {
                    *s = a;
                }
            }
            if let Some(rgb) = self.rgb.as_mut() {
                for c in 0..3usize {
                    let (from, to) = (j.saturating_mul(3).saturating_add(c), k.saturating_mul(3).saturating_add(c));
                    if let Some(a) = rgb.get(from).copied()
                        && let Some(s) = rgb.get_mut(to)
                    {
                        *s = a;
                    }
                }
            }
            if let Some(v) = self.inten.as_mut()
                && let Some(a) = v.get(j).copied()
                && let Some(s) = v.get_mut(k)
            {
                *s = a;
            }
            if let Some(v) = self.cls.as_mut()
                && let Some(a) = v.get(j).copied()
                && let Some(s) = v.get_mut(k)
            {
                *s = a;
            }
            j = j.saturating_add(2);
            k = k.saturating_add(1);
        }
        self.n = k;
        self.rate /= 2.0;
    }
    #[allow(clippy::too_many_arguments)]
    fn put(&mut self, x: f64, y: f64, z: f64, r: u16, g: u16, b: u16, i: u16, c: u8) {
        let k = self.n;
        self.n = self.n.saturating_add(1);
        if let Some(s) = self.x.get_mut(k) {
            *s = x;
        }
        if let Some(s) = self.y.get_mut(k) {
            *s = y;
        }
        if let Some(s) = self.z.get_mut(k) {
            *s = z;
        }
        if let Some(v) = self.rgb.as_mut() {
            let o = k.saturating_mul(3);
            for (d, val) in [r, g, b].into_iter().enumerate() {
                if let Some(s) = v.get_mut(o.saturating_add(d)) {
                    *s = val;
                }
            }
        }
        if let Some(s) = self.inten.as_mut().and_then(|v| v.get_mut(k)) {
            *s = i;
        }
        if let Some(s) = self.cls.as_mut().and_then(|v| v.get_mut(k)) {
            *s = c;
        }
    }
}

/// `Bounds`
struct Bounds {
    min: [f64; 3],
    max: [f64; 3],
}
impl Bounds {
    fn new() -> Bounds {
        Bounds { min: [f64::INFINITY; 3], max: [f64::NEG_INFINITY; 3] }
    }
    fn add(&mut self, p: [f64; 3]) {
        for ((lo, hi), v) in self.min.iter_mut().zip(self.max.iter_mut()).zip(p) {
            if v < *lo {
                *lo = v;
            }
            if v > *hi {
                *hi = v;
            }
        }
    }
}

/// A loaded cloud (`loadCloud`'s result).
#[derive(Clone, Debug, PartialEq)]
pub struct Cloud {
    pub name: String,
    pub kind: Kind,
    /// points read
    pub read: f64,
    /// points kept
    pub n: usize,
    pub origin: [f64; 3],
    /// f32 offsets from `origin`, 3 per point
    pub pos: Vec<f32>,
    pub rgb: Option<Vec<u8>>,
    pub inten: Option<Vec<u16>>,
    pub cls: Option<Vec<u8>>,
    /// of every point read (absolute)
    pub min: [f64; 3],
    pub max: [f64; 3],
    /// points read per class (`classCounts`)
    pub class_counts: Vec<f64>,
    pub unit: Option<String>,
    pub z_unit: Option<String>,
    pub notes: Vec<String>,
}

impl Cloud {
    /// `classCounts.size`
    pub fn classes_seen(&self) -> usize {
        self.class_counts.iter().filter(|n| **n > 0.0).count()
    }
    /// `pos[i * 3 + a]` as a double.
    pub fn p(&self, i: usize, a: usize) -> f64 {
        i.checked_mul(3).and_then(|o| o.checked_add(a)).and_then(|o| self.pos.get(o)).map_or(f64::NAN, |v| f64::from(*v))
    }
}

/// `loadCloud(file, info, { budget })`: read every point, keep at most `budget` (1000 … 20 million).
pub fn load_cloud(tr: &Catalog, file: &InFile, info: &Info, budget: f64) -> R<Cloud> {
    let budget = budget.floor().clamp(1000.0, BUDGET_MAX);
    let rate = if info.count > 0.0 { budget / info.count } else { 1.0 };
    let mut keep = Keeper::new(budget, rate, info.color, info.intensity, info.classes);
    let mut bx = Bounds::new();
    let mut class_counts = vec![0.0f64; 256];
    let mut notes = info.notes.clone();
    let read = match info.kind {
        Kind::Xyz => scan_text(tr, file, info, &mut keep, &mut bx, &mut notes),
        _ => scan_las(file, info, &mut keep, &mut bx, &mut class_counts, &mut notes)?,
    };
    if keep.n == 0 {
        return Err(tr.t("pc.empty", &[("name", file.name.clone())]));
    }
    Ok(finish(tr, file, info, keep, &bx, class_counts, read, notes))
}

#[allow(clippy::too_many_arguments)]
fn finish(
    tr: &Catalog,
    file: &InFile,
    info: &Info,
    keep: Keeper,
    bx: &Bounds,
    class_counts: Vec<f64>,
    read: f64,
    mut notes: Vec<String>,
) -> Cloud {
    let n = keep.n;
    let origin = [bx.min[0].floor(), bx.min[1].floor(), bx.min[2].floor()];
    let mut pos = Vec::with_capacity(n.saturating_mul(3));
    for k in 0..n {
        for (a, v) in [&keep.x, &keep.y, &keep.z].into_iter().enumerate() {
            let o = origin.get(a).copied().unwrap_or(0.0);
            pos.push((v.get(k).copied().unwrap_or(f64::NAN) - o) as f32);
        }
    }
    let mut rgb = None;
    if let Some(src) = &keep.rgb {
        let used = src.get(..n.saturating_mul(3)).unwrap_or(src);
        let hi = used.iter().copied().max().unwrap_or(0);
        if hi == 0 {
            notes.push(tr.s("pc.no_color"));
        } else {
            let shift = if hi > 255 { 8 } else { 0 };
            rgb = Some(used.iter().map(|v| (v >> shift) as u8).collect());
        }
    }
    if read < info.count && info.kind != Kind::Xyz {
        notes.push(tr.t("pc.read_short", &[("read", locale_int(read)), ("count", locale_int(info.count))]));
    }
    Cloud {
        name: file.name.clone(),
        kind: info.kind,
        read,
        n,
        origin,
        pos,
        rgb,
        inten: keep.inten.map(|mut v| {
            v.truncate(n);
            v
        }),
        cls: keep.cls.map(|mut v| {
            v.truncate(n);
            v
        }),
        min: bx.min,
        max: bx.max,
        class_counts,
        unit: info.unit.clone(),
        z_unit: info.z_unit.clone().or_else(|| info.unit.clone()),
        notes,
    }
}

fn le_i32(b: &[u8], o: usize) -> f64 {
    b.get(o..o.saturating_add(4)).and_then(|s| <[u8; 4]>::try_from(s).ok()).map_or(f64::NAN, |a| f64::from(i32::from_le_bytes(a)))
}
fn le_u16(b: &[u8], o: usize) -> u16 {
    b.get(o..o.saturating_add(2)).and_then(|s| <[u8; 2]>::try_from(s).ok()).map_or(0, u16::from_le_bytes)
}

fn scan_las(file: &InFile, info: &Info, keep: &mut Keeper, bx: &mut Bounds, class_counts: &mut [f64], notes: &mut Vec<String>) -> R<f64> {
    let Some(h) = info.las.as_ref() else { return Ok(0.0) };
    let ext = h.format >= 6.0;
    let l = h.record_length;
    let ca = color_at(h.format).filter(|c| (*c as f64) + 6.0 <= l);
    let [sx, sy, sz] = h.scale;
    let [ox, oy, oz] = h.offset;
    let mut on_rec = |rec: &[u8]| -> R<()> {
        let x = le_i32(rec, 0) * sx + ox;
        let y = le_i32(rec, 4) * sy + oy;
        let z = le_i32(rec, 8) * sz + oz;
        let c = if ext { rec.get(16).copied().unwrap_or(0) } else { rec.get(15).copied().unwrap_or(0) & 31 };
        bx.add([x, y, z]);
        if let Some(s) = class_counts.get_mut(usize::from(c)) {
            *s += 1.0;
        }
        if !keep.want() {
            return Ok(());
        }
        let i = le_u16(rec, 12);
        match ca {
            Some(a) => keep.put(x, y, z, le_u16(rec, a), le_u16(rec, a.saturating_add(2)), le_u16(rec, a.saturating_add(4)), i, c),
            None => keep.put(x, y, z, 0, 0, 0, i, c),
        }
        Ok(())
    };
    scan_records(file, h, notes, &mut on_rec)
}

fn clamp16(v: f64) -> u16 {
    round(v).clamp(0.0, 65535.0) as u16
}

fn scan_text(tr: &Catalog, file: &InFile, info: &Info, keep: &mut Keeper, bx: &mut Bounds, notes: &mut Vec<String>) -> f64 {
    let cols = info.columns.unwrap_or(text_columns(3));
    let text = decode_utf8(&file.bytes);
    let (mut read, mut skipped) = (0.0f64, 0usize);
    let mut first = true;
    let mut v: Vec<f64> = Vec::with_capacity(8);
    for s in text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)) {
        let ok = numbers_into(s, &mut v);
        if first {
            first = false;
            if ok
                && let [one] = v.as_slice()
                && one.fract() == 0.0
            {
                continue;
            }
        }
        if !ok || v.len() < 3 {
            if !cc_civil::js::trim(s).is_empty() {
                skipped = skipped.saturating_add(1);
            }
            continue;
        }
        let (x, y, z) = (v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0), v.get(2).copied().unwrap_or(0.0));
        bx.add([x, y, z]);
        read += 1.0;
        if !keep.want() {
            continue;
        }
        let i = match cols.i {
            Some(ci) if v.len() > ci => clamp16(v.get(ci).copied().unwrap_or(0.0)),
            _ => 0,
        };
        match cols.rgb {
            Some(c) if v.len() > c.saturating_add(2) => {
                let g = |o: usize| clamp16(v.get(c.saturating_add(o)).copied().unwrap_or(0.0));
                keep.put(x, y, z, g(0), g(1), g(2), i, 0);
            }
            _ => keep.put(x, y, z, 0, 0, 0, i, 0),
        }
    }
    if skipped > 0 {
        notes.push(tr.t("pc.text.skipped", &[("n", skipped.to_string()), ("count", locale_int(skipped as f64))]));
    }
    read
}

// --------------------------------------------------------------------- colours

const RAMP: [[f64; 3]; 5] = [[0.19, 0.29, 0.75], [0.15, 0.62, 0.85], [0.2, 0.7, 0.35], [0.95, 0.8, 0.2], [0.85, 0.25, 0.15]];

fn ramp_at(f: f64, out: &mut [u8; 3]) {
    let x = f.clamp(0.0, 1.0) * 4.0;
    let i = x.floor().min(3.0);
    let u = x - i;
    let (a, b) = match i as i64 {
        0 => (RAMP[0], RAMP[1]),
        1 => (RAMP[1], RAMP[2]),
        2 => (RAMP[2], RAMP[3]),
        _ => (RAMP[3], RAMP[4]),
    };
    for ((s, av), bv) in out.iter_mut().zip(a).zip(b) {
        *s = round((av + (bv - av) * u) * 255.0) as u8;
    }
}

/// `CLASS_COLORS`: ASPRS classes as colours.
pub fn class_color(c: u8) -> [u8; 3] {
    match c {
        0 => [150, 150, 150],
        1 => [170, 170, 170],
        2 => [160, 120, 70],
        3 => [140, 200, 110],
        4 => [70, 160, 60],
        5 => [30, 110, 40],
        6 => [215, 70, 50],
        7 => [110, 110, 110],
        8 => [240, 200, 40],
        9 => [50, 110, 220],
        10 => [120, 80, 160],
        11 => [90, 90, 100],
        12 => [200, 200, 120],
        13 => [230, 150, 40],
        14 => [240, 120, 20],
        15 => [180, 60, 160],
        16 => [220, 160, 60],
        17 => [140, 140, 190],
        18 => [80, 80, 80],
        _ => [200, 200, 200],
    }
}

/// `quantile(arr, step, offset, frac, n)`: a value below which `frac` of the (sampled) values lie.
fn quantile(get: &dyn Fn(usize) -> f64, frac: f64, n: usize) -> f64 {
    let every = (n / 20000).max(1);
    let mut pick: Vec<f64> = (0..n).step_by(every).map(get).collect();
    pick.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let last = pick.len().saturating_sub(1);
    let at = (frac * last as f64).floor().max(0.0);
    let i = usize_of(at).unwrap_or(0).min(last);
    pick.get(i).copied().unwrap_or(f64::NAN)
}

/// `colorize(cloud, mode, { color })` → 3 bytes per point.
pub fn colorize(cloud: &Cloud, mode: &str, color: [u8; 3]) -> Vec<u8> {
    let n = cloud.n;
    let mut out = vec![0u8; n.saturating_mul(3)];
    if mode == "rgb"
        && let Some(rgb) = &cloud.rgb
    {
        for (o, v) in out.iter_mut().zip(rgb) {
            *o = *v;
        }
        return out;
    }
    if mode == "single" {
        for px in out.as_chunks_mut::<3>().0 {
            px.copy_from_slice(&color);
        }
        return out;
    }
    if mode == "class"
        && let Some(cls) = &cloud.cls
    {
        for (px, c) in out.as_chunks_mut::<3>().0.iter_mut().zip(cls) {
            px.copy_from_slice(&class_color(*c));
        }
        return out;
    }
    if mode == "intensity"
        && let Some(inten) = &cloud.inten
    {
        let get = |k: usize| inten.get(k).map_or(f64::NAN, |v| f64::from(*v));
        let lo = quantile(&get, 0.02, n);
        let hi0 = quantile(&get, 0.98, n);
        let hi = if hi0 == 0.0 || hi0.is_nan() { 1.0 } else { hi0 };
        let span = (hi - lo).max(1.0);
        for (px, v) in out.as_chunks_mut::<3>().0.iter_mut().zip(inten) {
            let g = round(40.0 + 215.0 * ((f64::from(*v) - lo) / span).clamp(0.0, 1.0)) as u8;
            px.copy_from_slice(&[g, g, g]);
        }
        return out;
    }
    // height (also rgb without colours, class without classes, intensity without intensity)
    let get = |k: usize| cloud.p(k, 2);
    let lo = quantile(&get, 0.01, n);
    let hi = quantile(&get, 0.99, n);
    let d = hi - lo;
    let span = if d == 0.0 || d.is_nan() { 1.0 } else { d };
    for (px, p) in out.as_chunks_mut::<3>().0.iter_mut().zip(cloud.pos.as_chunks::<3>().0) {
        ramp_at((f64::from(p[2]) - lo) / span, px);
    }
    out
}

/// `colorModes(cloud)`: its own colours, intensity and classes only when the file has them.
pub fn color_modes(cloud: &Cloud) -> Vec<&'static str> {
    ["rgb", "height", "intensity", "class", "single"]
        .into_iter()
        .filter(|m| match *m {
            "rgb" => cloud.rgb.is_some(),
            "intensity" => cloud.inten.is_some(),
            "class" => cloud.cls.is_some() && cloud.classes_seen() > 1,
            _ => true,
        })
        .collect()
}

// --------------------------------------------------------------------- slices, snapping

/// `sliceOf(cloud, { axis, at, thick })`: the points within thick / 2 of `at` (absolute file units) along an axis.
pub fn slice_of(cloud: &Cloud, axis: usize, at: f64, thick: f64) -> Vec<u32> {
    let a = axis.min(2);
    let c = at - cloud.origin.get(a).copied().unwrap_or(0.0);
    let h = thick.abs() / 2.0;
    let mut out = Vec::new();
    for k in 0..cloud.n {
        if (cloud.p(k, a) - c).abs() <= h
            && let Ok(k) = u32::try_from(k)
        {
            out.push(k);
        }
    }
    out
}

/// `pointAt(cloud, i)`: absolute file units.
pub fn point_at(cloud: &Cloud, i: usize) -> [f64; 3] {
    [cloud.p(i, 0) + cloud.origin[0], cloud.p(i, 1) + cloud.origin[1], cloud.p(i, 2) + cloud.origin[2]]
}

/// A bucket grid in plan (x, y) of some of a cloud's points (or all), for the nearest point within a radius.
///
/// The JS keys a `Map` by `i * 1048576 + j`; here the cells live in one dense table (counts, then the points in
/// their order) when the points' cells span at most [`DENSE_CELLS`], else in a hash map with the JS's keys. Points
/// sit at x, y ≥ 0 from the origin, so their cells' j are never negative: a query cell that the JS's key would fold
/// onto another row lies over a million cells away, where no point is within the radius — the answers are the same.
#[derive(Clone, Debug)]
pub struct PlanGrid {
    pub cell: f64,
    cells: Cells,
    pub size: usize,
}

/// At most this many cells in the dense table (16 MB of offsets).
pub const DENSE_CELLS: usize = 4 << 20;

#[derive(Clone, Debug)]
enum Cells {
    /// cells (i0.., j0..) of `ni` × `nj`: `start[c]..start[c + 1]` in `idx`
    Dense {
        i0: f64,
        j0: f64,
        ni: usize,
        nj: usize,
        start: Vec<u32>,
        idx: Vec<u32>,
    },
    Map(HashMap<i64, Vec<u32>>),
}

/// `key(i, j) = i * 1048576 + j` (cells of other rows may share a key, as in the JS)
fn grid_key(i: f64, j: f64) -> Option<i64> {
    let k = i * 1048576.0 + j;
    if k.is_finite() && k.abs() < 9.0e15 { Some(k as i64) } else { None }
}

impl PlanGrid {
    pub fn new(cloud: &Cloud, indices: Option<&[u32]>, cell: f64) -> PlanGrid {
        let count = indices.map_or(cloud.n, <[u32]>::len);
        let span = (cloud.max[0] - cloud.min[0]).max(cloud.max[1] - cloud.min[1]).max(1e-9);
        let cell = if cell > 0.0 { cell } else { (span / 1024.0).max(span / (count.max(1) as f64).sqrt() * 2.0) };
        let all: Vec<u32>;
        let ix: &[u32] = match indices {
            Some(ix) => ix,
            None => {
                all = (0..cloud.n).filter_map(|k| u32::try_from(k).ok()).collect();
                &all
            }
        };
        let ij = |k: u32| ((cloud.p(k as usize, 0) / cell).floor(), (cloud.p(k as usize, 1) / cell).floor());
        // the cells' range
        let (mut i0, mut i1, mut j0, mut j1) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
        for &k in ix {
            let (i, j) = ij(k);
            i0 = i0.min(i);
            i1 = i1.max(i);
            j0 = j0.min(j);
            j1 = j1.max(j);
        }
        let ni = i1 - i0 + 1.0;
        let nj = j1 - j0 + 1.0;
        let dense = ix.is_empty() || (ni.is_finite() && nj.is_finite() && j0 >= 0.0 && j1 < 1048576.0 && ni * nj <= DENSE_CELLS as f64);
        if dense && !ix.is_empty() {
            let (ni, nj) = (ni as usize, nj as usize);
            let cell_of = |k: u32| {
                let (i, j) = ij(k);
                ((i - i0) as usize).saturating_mul(nj).saturating_add((j - j0) as usize)
            };
            let total = ni.saturating_mul(nj);
            let mut start = vec![0u32; total.saturating_add(1)];
            for &k in ix {
                if let Some(c) = start.get_mut(cell_of(k).saturating_add(1)) {
                    *c = c.saturating_add(1);
                }
            }
            let mut acc = 0u32;
            for c in &mut start {
                acc = acc.saturating_add(*c);
                *c = acc;
            }
            let mut fill: Vec<u32> = start.clone();
            let mut idx = vec![0u32; ix.len()];
            for &k in ix {
                if let Some(f) = fill.get_mut(cell_of(k)) {
                    if let Some(slot) = idx.get_mut(*f as usize) {
                        *slot = k;
                    }
                    *f = f.saturating_add(1);
                }
            }
            return PlanGrid { cell, cells: Cells::Dense { i0, j0, ni, nj, start, idx }, size: count };
        }
        let mut map: HashMap<i64, Vec<u32>> = HashMap::new();
        for &k in ix {
            let (i, j) = ij(k);
            if let Some(key) = grid_key(i, j) {
                map.entry(key).or_default().push(k);
            }
        }
        PlanGrid { cell, cells: Cells::Map(map), size: count }
    }

    /// The points of cell (i, j) in their order.
    fn bucket(&self, i: f64, j: f64) -> &[u32] {
        match &self.cells {
            Cells::Dense { i0, j0, ni, nj, start, idx } => {
                let (di, dj) = (i - i0, j - j0);
                if di < 0.0 || dj < 0.0 || di >= *ni as f64 || dj >= *nj as f64 {
                    return &[];
                }
                let c = (di as usize).saturating_mul(*nj).saturating_add(dj as usize);
                match (start.get(c), start.get(c.saturating_add(1))) {
                    (Some(a), Some(b)) => idx.get(*a as usize..*b as usize).unwrap_or(&[]),
                    _ => &[],
                }
            }
            Cells::Map(m) => grid_key(i, j).and_then(|k| m.get(&k)).map_or(&[], Vec::as_slice),
        }
    }

    /// `nearest(x, y, r)`: the point nearest (x, y) within r (absolute file units), or None.
    pub fn nearest(&self, cloud: &Cloud, x: f64, y: f64, r: f64) -> Option<usize> {
        let lx = x - cloud.origin[0];
        let ly = y - cloud.origin[1];
        let i0 = ((lx - r) / self.cell).floor();
        let i1 = ((lx + r) / self.cell).floor();
        let j0 = ((ly - r) / self.cell).floor();
        let j1 = ((ly + r) / self.cell).floor();
        let cells = (i1 - i0 + 1.0) * (j1 - j0 + 1.0);
        // a radius far bigger than the points: nothing sensible to snap to (a non-finite one too: the JS would loop)
        if cells > 40000.0 || !cells.is_finite() {
            return None;
        }
        let mut best = None;
        let mut bd = r * r;
        let mut i = i0;
        while i <= i1 {
            let mut j = j0;
            while j <= j1 {
                for k in self.bucket(i, j) {
                    let kk = *k as usize;
                    let dx = cloud.p(kk, 0) - lx;
                    let dy = cloud.p(kk, 1) - ly;
                    let d = dx * dx + dy * dy;
                    if d <= bd {
                        bd = d;
                        best = Some(kk);
                    }
                }
                j += 1.0;
            }
            i += 1.0;
        }
        best
    }
}

/// `cloudSummary(cloud)`
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub read: f64,
    pub kept: usize,
    pub size: [f64; 3],
    pub classes: Vec<(u8, String, f64)>,
}

pub fn cloud_summary(tr: &Catalog, cloud: &Cloud) -> Summary {
    let classes = cloud
        .class_counts
        .iter()
        .enumerate()
        .filter(|(_, n)| **n > 0.0)
        .filter_map(|(c, n)| {
            let c8 = u8::try_from(c).ok()?;
            let name = class_name(f64::from(c8)).unwrap_or_else(|| tr.t("pc.class.other", &[("c", c.to_string())]));
            Some((c8, name, *n))
        })
        .collect();
    Summary {
        read: cloud.read,
        kept: cloud.n,
        size: [cloud.max[0] - cloud.min[0], cloud.max[1] - cloud.min[1], cloud.max[2] - cloud.min[2]],
        classes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat() -> Catalog {
        Catalog::new(&serde_json::json!({}))
    }

    #[test]
    fn text_clouds_keep_their_columns_and_counts() {
        let f = InFile { name: "a.pts".into(), bytes: b"3\n1 2 3 10 255 0 0\n4 5 6 20 0 255 0\nnope\n7,8,9,30,0,0,255\n".to_vec() };
        let info = read_cloud_info(&cat(), &f).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(info.count, 3.0);
        assert_eq!(info.columns, Some(Columns { n: 7, i: Some(3), rgb: Some(4) }));
        let c = load_cloud(&cat(), &f, &info, 2e6).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(c.n, 3);
        assert_eq!(c.origin, [1.0, 2.0, 3.0]);
        assert_eq!(c.rgb.as_deref(), Some(&[255u8, 0, 0, 0, 255, 0, 0, 0, 255][..]));
        assert_eq!(c.notes, vec!["pc.text.skipped".to_string()]);
        let g = PlanGrid::new(&c, None, 0.0);
        assert_eq!(g.nearest(&c, 4.1, 5.1, 1.0), Some(1));
        assert_eq!(g.nearest(&c, 40.0, 50.0, 1.0), None);
        assert_eq!(g.nearest(&c, 4.0, 5.0, f64::INFINITY), None);
        assert_eq!(slice_of(&c, 2, 6.0, 1.0), vec![1]);
    }

    #[test]
    fn not_a_cloud_and_empty_files_are_refused() {
        assert!(read_cloud_info(&cat(), &InFile { name: "a.txt".into(), bytes: vec![1, 2] }).is_err());
        assert!(read_cloud_info(&cat(), &InFile { name: "a.xyz".into(), bytes: b"x y z\n".to_vec() }).is_err());
        assert!(read_cloud_info(&cat(), &InFile { name: "a.las".into(), bytes: b"LASF".to_vec() }).is_err());
    }

    #[test]
    fn the_keeper_thins_evenly() {
        let mut k = Keeper::new(1000.0, 1.0, false, false, false);
        for i in 0..5000 {
            if k.want() {
                k.put(f64::from(i), 0.0, 0.0, 0, 0, 0, 0, 0);
            }
        }
        assert!(k.n <= 1000 && k.n > 500);
        assert_eq!(k.x.first().copied(), Some(0.0));
    }
}
