//! geotiff-write.js — a surface out as a GeoTIFF elevation grid: the grid laid out in grid coordinates and sampled
//! from the TIN, then a classic little-endian TIFF with one Float32 band, tiles or strips, DEFLATE or none, GeoKeys,
//! GDAL_NODATA and GDAL_METADATA. Uncompressed files are byte-for-byte the JS's; DEFLATE blocks are zlib streams
//! (miniz_oxide, level 6) that inflate to the same bytes (the JS's CompressionStream is zlib itself).

use cc_civil::i18n::t;
use cc_civil::js::{hypot, max, min, num_to_string, to_fixed};
use cc_civil::tin::Surface;
use cc_civil::tr;

type R<T> = Result<T, String>;

/// `NODATA` (float32's lowest value).
pub const NODATA: f32 = f32::MIN;
const NODATA_TEXT: &str = "-3.4028234663852886e+38";

/// `averageEdge(surface)`.
pub fn average_edge(s: &Surface) -> f64 {
    let e = s.edges();
    if e.is_empty() {
        return 0.0;
    }
    let mut sum = 0.0;
    for p in e.as_chunks::<2>().0 {
        let (a, b) = (p[0] as usize, p[1] as usize);
        let (xa, xb, ya, yb) = (s.x.as_vec().get(a), s.x.as_vec().get(b), s.y.as_vec().get(a), s.y.as_vec().get(b));
        let g = |v: Option<&f64>| v.copied().unwrap_or(f64::NAN);
        sum += hypot(g(xa) - g(xb), g(ya) - g(yb));
    }
    sum / (e.len() as f64 / 2.0)
}

/// `surfaceGrid(...)` result.
#[derive(Clone, Debug, PartialEq)]
pub struct DemGrid {
    pub width: f64,
    pub height: f64,
    pub gt: [f64; 6],
    pub values: Vec<f32>,
}

/// `surfaceGrid(surface, { cell, toGrid, fromGrid, maxCells })`.
pub fn surface_grid(
    s: &Surface,
    cell: f64,
    to_grid: &dyn Fn(f64, f64) -> [f64; 2],
    from_grid: &dyn Fn(f64, f64) -> [f64; 2],
    max_cells: f64,
) -> R<DemGrid> {
    if !(cell > 0.0) || !cell.is_finite() {
        return Err(t("gtw.cell.bad", &[]));
    }
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for lp in s.outline() {
        for p in lp {
            let [gx, gy] = to_grid(p.x, p.y);
            min_x = min(min_x, gx);
            max_x = max(max_x, gx);
            min_y = min(min_y, gy);
            max_y = max(max_y, gy);
        }
    }
    if !min_x.is_finite() {
        return Err(t("gtw.no_outline", &[]));
    }
    let x0 = (min_x / cell).floor() * cell;
    let y0 = (max_y / cell).ceil() * cell;
    let width = max(1.0, ((max_x - x0) / cell - 1e-9).ceil());
    let height = max(1.0, ((y0 - min_y) / cell - 1e-9).ceil());
    if width * height > max_cells {
        return Err(tr!(
            "gtw.too_many",
            w = crate::raster::locale_int(width),
            h = crate::raster::locale_int(height),
            cells = to_fixed(width * height / 1e6, 1),
            max = num_to_string(max_cells / 1e6)
        ));
    }
    let (wu, hu) = (crate::terrain::count_of(width), crate::terrain::count_of(height));
    let mut values = vec![0f32; wu.saturating_mul(hu)];
    for r in 0..hu {
        let gy = y0 - (r as f64 + 0.5) * cell;
        for c in 0..wu {
            let [x, y] = from_grid(x0 + (c as f64 + 0.5) * cell, gy);
            let z = s.z_at(x, y);
            if let Some(slot) = values.get_mut(r.saturating_mul(wu).saturating_add(c)) {
                // a Float32Array store
                #[allow(clippy::cast_possible_truncation)]
                let v = match z {
                    None => NODATA,
                    Some(z) => z as f32,
                };
                *slot = v;
            }
        }
    }
    Ok(DemGrid { width, height, gt: [x0, cell, 0.0, y0, 0.0, -cell], values })
}

/// `writeDemTiff` options.
#[derive(Clone, Debug)]
pub struct DemOptions {
    pub epsg: Option<f64>,
    pub citation: String,
    pub xy_unit: String,
    pub z_unit: String,
    pub description: String,
    pub tiled: bool,
    pub tile: f64,
    pub deflate: bool,
}

/// `ToUint16(v)`.
fn u16_of(v: f64) -> u16 {
    if !v.is_finite() {
        return 0;
    }
    let m = v.trunc().rem_euclid(65536.0);
    u16::try_from(cc_civil::js::usize_of(m).unwrap_or(0)).unwrap_or(0)
}
/// `ToUint32(v)`.
fn u32_of(v: f64) -> u32 {
    if !v.is_finite() {
        return 0;
    }
    let m = v.trunc().rem_euclid(4294967296.0);
    u32::try_from(cc_civil::js::usize_of(m).unwrap_or(0)).unwrap_or(0)
}

struct Tag {
    tag: u16,
    ty: u16,
    count: u32,
    data: Vec<u8>,
    at: u32,
}

/// `enc(s)`: printable ASCII and newlines kept, every other UTF-16 unit → '?', then a NUL.
fn enc(s: &str) -> Vec<u8> {
    let mut out: Vec<u8> =
        s.encode_utf16().map(|u| if (0x20..=0x7e).contains(&u) || u == 0x0a { u8::try_from(u).unwrap_or(b'?') } else { b'?' }).collect();
    out.push(0);
    out
}
fn xml_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
fn unit_type(u: &str) -> &str {
    match u {
        "m" => "m",
        "ft" => "ft",
        "ftUS" => "US survey foot",
        _ => "undefined",
    }
}
fn unit_epsg(u: &str) -> u16 {
    match u {
        "ft" => 9002,
        "ftUS" => 9003,
        _ => 9001,
    }
}

/// `writeDemTiff({...})`.
pub fn write_dem_tiff(width: f64, height: f64, gt: [f64; 6], values: &[f32], o: &DemOptions) -> R<Vec<u8>> {
    if !(cc_civil::js::is_integer(width) && cc_civil::js::is_integer(height) && width > 0.0 && height > 0.0)
        || values.len() as f64 != width * height
    {
        return Err(t("gtw.size_wrong", &[]));
    }
    if gt[2] != 0.0 || gt[4] != 0.0 {
        return Err(t("gtw.north_up", &[]));
    }
    let bw = if o.tiled { o.tile } else { width };
    let bh = if o.tiled { o.tile } else { min(height, 16.0) };
    if !(bw >= 1.0) || !(bh >= 1.0) || !bw.is_finite() || !bh.is_finite() {
        return Err("Invalid typed array length".into());
    }
    let across = (width / bw).ceil();
    let down = (height / bh).ceil();
    let (wu, bwu, bhu) = (crate::terrain::count_of(width), crate::terrain::count_of(bw.trunc()), crate::terrain::count_of(bh.trunc()));
    let hu = crate::terrain::count_of(height);
    let mut blocks: Vec<Vec<u8>> = Vec::new();
    let mut by = 0usize;
    while (by as f64) < down {
        let mut bx = 0usize;
        while (bx as f64) < across {
            let rows = if o.tiled { bhu } else { bhu.min(hu.saturating_sub(by.saturating_mul(bhu))) };
            let cols = if o.tiled { bwu } else { wu };
            let n = rows.checked_mul(cols).filter(|n| *n <= 1 << 28).ok_or("Invalid typed array length")?;
            let mut buf = vec![if o.tiled { NODATA } else { 0.0f32 }; n];
            for r in 0..rows {
                let sr = by.saturating_mul(bhu).saturating_add(r);
                if sr >= hu {
                    break;
                }
                let sc = bx.saturating_mul(bwu);
                let k = cols.min(wu.saturating_sub(sc));
                let from = sr.saturating_mul(wu).saturating_add(sc);
                if let (Some(src), Some(dst)) = (
                    values.get(from..from.saturating_add(k)),
                    buf.get_mut(r.saturating_mul(cols)..r.saturating_mul(cols).saturating_add(k)),
                ) {
                    dst.copy_from_slice(src);
                }
            }
            let mut bytes: Vec<u8> = Vec::with_capacity(n.saturating_mul(4));
            for v in &buf {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
            if o.deflate {
                bytes = miniz_oxide::deflate::compress_to_vec_zlib(&bytes, 6);
            }
            blocks.push(bytes);
            bx = bx.saturating_add(1);
        }
        by = by.saturating_add(1);
    }
    let mut tags: Vec<Tag> = Vec::new();
    let short = |tags: &mut Vec<Tag>, tag: u16, vals: &[f64]| {
        let data: Vec<u8> = vals.iter().flat_map(|v| u16_of(*v).to_le_bytes()).collect();
        tags.push(Tag { tag, ty: 3, count: u32::try_from(vals.len()).unwrap_or(0), data, at: 0 });
    };
    let long = |tags: &mut Vec<Tag>, tag: u16, vals: &[f64]| {
        let data: Vec<u8> = vals.iter().flat_map(|v| u32_of(*v).to_le_bytes()).collect();
        tags.push(Tag { tag, ty: 4, count: u32::try_from(vals.len()).unwrap_or(0), data, at: 0 });
    };
    let dbl = |tags: &mut Vec<Tag>, tag: u16, vals: &[f64]| {
        let data: Vec<u8> = vals.iter().flat_map(|v| v.to_le_bytes()).collect();
        tags.push(Tag { tag, ty: 12, count: u32::try_from(vals.len()).unwrap_or(0), data, at: 0 });
    };
    let asc = |tags: &mut Vec<Tag>, tag: u16, s: &str| {
        let data = enc(s);
        tags.push(Tag { tag, ty: 2, count: u32::try_from(data.len()).unwrap_or(0), data, at: 0 });
    };
    long(&mut tags, 256, &[width]);
    long(&mut tags, 257, &[height]);
    short(&mut tags, 258, &[32.0]);
    short(&mut tags, 259, &[if o.deflate { 8.0 } else { 1.0 }]);
    short(&mut tags, 262, &[1.0]);
    if !o.description.is_empty() {
        asc(&mut tags, 270, &o.description);
    }
    short(&mut tags, 277, &[1.0]);
    if !o.tiled {
        long(&mut tags, 278, &[bh]);
    }
    short(&mut tags, 284, &[1.0]);
    asc(&mut tags, 305, "CivilCraft");
    if o.tiled {
        long(&mut tags, 322, &[bw]);
        long(&mut tags, 323, &[bh]);
    }
    short(&mut tags, 339, &[3.0]);
    dbl(&mut tags, 33550, &[gt[1], -gt[5], 0.0]);
    dbl(&mut tags, 33922, &[0.0, 0.0, 0.0, gt[0], gt[3], 0.0]);
    if let Some(epsg) = o.epsg.filter(|e| *e != 0.0 && !e.is_nan()) {
        let base = if o.citation.is_empty() { format!("EPSG:{}", num_to_string(epsg)) } else { o.citation.clone() };
        let cit = format!("{}|", base.replace('|', "/"));
        let cl = cit.encode_utf16().count() as f64;
        let keys: [[f64; 4]; 5] = [
            [1024.0, 0.0, 1.0, 1.0],
            [1025.0, 0.0, 1.0, 1.0],
            [1026.0, 34737.0, cl, 0.0],
            [3072.0, 0.0, 1.0, epsg],
            [3076.0, 0.0, 1.0, f64::from(unit_epsg(&o.xy_unit))],
        ];
        let mut v = vec![1.0, 1.0, 0.0, keys.len() as f64];
        for k in keys {
            v.extend_from_slice(&k);
        }
        short(&mut tags, 34735, &v);
        asc(&mut tags, 34737, &cit);
    }
    let first = o.description.split('.').next().unwrap_or("");
    let ut = unit_type(&o.z_unit);
    let meta = format!(
        "<GDALMetadata>\n  <Item name=\"DESCRIPTION\" sample=\"0\" role=\"description\">{}</Item>\n  <Item name=\"UNITTYPE\" sample=\"0\" role=\"unittype\">{ut}</Item>\n  <Item name=\"ELEVATION_UNITS\">{ut}</Item>\n</GDALMetadata>",
        xml_text(if first.is_empty() { "Elevation" } else { first })
    );
    asc(&mut tags, 42112, &meta);
    asc(&mut tags, 42113, NODATA_TEXT);
    let (off_tag, cnt_tag) = if o.tiled { (324, 325) } else { (273, 279) };
    let zeros = vec![0.0; blocks.len()];
    long(&mut tags, off_tag, &zeros);
    let lens: Vec<f64> = blocks.iter().map(|b| b.len() as f64).collect();
    long(&mut tags, cnt_tag, &lens);
    tags.sort_by_key(|t| t.tag);
    let ifd_size = 2usize.saturating_add(tags.len().saturating_mul(12)).saturating_add(4);
    let mut p = 8usize.saturating_add(ifd_size);
    for tg in &mut tags {
        let sz = tg.data.len();
        if sz > 4 {
            tg.at = u32::try_from(p).map_err(|_| t("gtw.over_4gb", &[]))?;
            p = p.saturating_add(sz).saturating_add(sz & 1);
        }
    }
    let data_start = p;
    let mut total = data_start;
    for b in &blocks {
        total = total.saturating_add(b.len()).saturating_add(b.len() & 1);
    }
    if total as f64 > 4e9 {
        return Err(t("gtw.over_4gb", &[]));
    }
    let mut out = vec![0u8; total];
    let w = |out: &mut Vec<u8>, at: usize, b: &[u8]| {
        if let Some(d) = out.get_mut(at..at.saturating_add(b.len())) {
            d.copy_from_slice(b);
        }
    };
    w(&mut out, 0, &[0x49, 0x49]);
    w(&mut out, 2, &42u16.to_le_bytes());
    w(&mut out, 4, &8u32.to_le_bytes());
    let mut offs = Vec::new();
    let mut q = data_start;
    for b in &blocks {
        offs.push(q);
        w(&mut out, q, b);
        q = q.saturating_add(b.len()).saturating_add(b.len() & 1);
    }
    if let Some(ot) = tags.iter_mut().find(|t| t.tag == off_tag) {
        ot.data = offs.iter().flat_map(|v| u32::try_from(*v).unwrap_or(0).to_le_bytes()).collect();
    }
    w(&mut out, 8, &u16::try_from(tags.len()).unwrap_or(0).to_le_bytes());
    for (i, tg) in tags.iter().enumerate() {
        let e = 10usize.saturating_add(i.saturating_mul(12));
        w(&mut out, e, &tg.tag.to_le_bytes());
        w(&mut out, e.saturating_add(2), &tg.ty.to_le_bytes());
        w(&mut out, e.saturating_add(4), &tg.count.to_le_bytes());
        if tg.data.len() <= 4 {
            w(&mut out, e.saturating_add(8), &tg.data);
        } else {
            w(&mut out, e.saturating_add(8), &tg.at.to_le_bytes());
            w(&mut out, tg.at as usize, &tg.data);
        }
    }
    w(&mut out, 10usize.saturating_add(tags.len().saturating_mul(12)), &0u32.to_le_bytes());
    Ok(out)
}
