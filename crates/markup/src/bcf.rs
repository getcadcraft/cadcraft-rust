//! bcf.js — BCF 2.1 (.bcfzip) export: a stored zip written here (CRC-32, UTF-8 names, DOS dates), XML text,
//! name-based topic GUIDs (UUID v5 over SHA-1), the extension schema, and per markup `markup.bcf`,
//! `viewpoint.bcfv` and `snapshot.png`. Byte for byte what the JS writes for the same input and export time.

use crate::i18n::{Fill, t};
use crate::js::{civil, ns, parse_date_in, str_to_number, to_fixed, to_iso_s};
use crate::model::{Markup, STATUSES, TYPE_IDS, is_type, type_name};
use cc_clock::Zone;

// a const block: evaluated by the compiler (an overflow or bad index there fails the build, never a run)
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
static CRC_TABLE: [u32; 256] = {
    let mut t = [0u32; 256];
    let mut n = 0u32;
    while n < 256 {
        let mut c = n;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[n as usize] = c;
        n += 1;
    }
    t
};

/// `crc32(bytes)`.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for b in bytes {
        let i = usize::from((c ^ u32::from(*b)).to_le_bytes()[0]);
        c = CRC_TABLE.get(i).copied().unwrap_or(0) ^ (c >> 8);
    }
    c ^ 0xffff_ffff
}

/// One file of a zip.
#[derive(Debug, Clone)]
pub struct ZipFile {
    pub name: String,
    pub data: Vec<u8>,
}

fn u16le(v: &mut Vec<u8>, x: u64) {
    v.extend_from_slice(&((x & 0xffff) as u16).to_le_bytes());
}
fn u32le(v: &mut Vec<u8>, x: u64) {
    v.extend_from_slice(&((x & 0xffff_ffff) as u32).to_le_bytes());
}

/// `zip(files, date)`: stored entries, UTF-8 names, the DOS time of `date_ms` on `zone`'s wall clock (the JS's
/// `getHours()` … are local time).
pub fn zip(files: &[ZipFile], date_ms: f64, zone: Zone) -> Vec<u8> {
    zip_with(files, date_ms, zone, false)
}

/// xlsx.js `zip(files, {compress, date})`: like [`zip`], and with `compress` each entry deflated (raw deflate)
/// when that makes it smaller. The deflated bytes are miniz's, not zlib's (the JS uses CompressionStream): same
/// contents, other bytes.
pub fn zip_with(files: &[ZipFile], date_ms: f64, zone: Zone, compress: bool) -> Vec<u8> {
    let (y, mo, d, h, mi, s, _) = civil(zone.local_ms(date_ms));
    let dos_time = (u64::from(h) << 11 | u64::from(mi) << 5 | u64::from(s) >> 1) & 0xffff;
    // (year − 1980) << 9 in 32-bit two's complement (a year before 1980 goes negative), then & 0xffff
    let year = y.saturating_sub(1980).cast_unsigned();
    let dos_date = ((year << 9) | (u64::from(mo) << 5) | u64::from(d)) & 0xffff;
    let mut locals: Vec<u8> = Vec::new();
    let mut centrals: Vec<u8> = Vec::new();
    let mut offset: u64 = 0;
    for f in files {
        let name = f.name.as_bytes();
        let crc = u64::from(crc32(&f.data));
        let packed =
            if compress { Some(miniz_oxide::deflate::compress_to_vec(&f.data, 6)).filter(|d| d.len() < f.data.len()) } else { None };
        let (body, method): (&[u8], u64) = match &packed {
            Some(d) => (d, 8),
            None => (&f.data, 0),
        };
        let (nl, dl, bl) = (name.len() as u64, f.data.len() as u64, body.len() as u64);
        u32le(&mut locals, 0x0403_4b50);
        u16le(&mut locals, 20);
        u16le(&mut locals, 0x0800);
        u16le(&mut locals, method);
        u16le(&mut locals, dos_time);
        u16le(&mut locals, dos_date);
        u32le(&mut locals, crc);
        u32le(&mut locals, bl);
        u32le(&mut locals, dl);
        u16le(&mut locals, nl);
        u16le(&mut locals, 0);
        locals.extend_from_slice(name);
        locals.extend_from_slice(body);
        u32le(&mut centrals, 0x0201_4b50);
        u16le(&mut centrals, 20);
        u16le(&mut centrals, 20);
        u16le(&mut centrals, 0x0800);
        u16le(&mut centrals, method);
        u16le(&mut centrals, dos_time);
        u16le(&mut centrals, dos_date);
        u32le(&mut centrals, crc);
        u32le(&mut centrals, bl);
        u32le(&mut centrals, dl);
        u16le(&mut centrals, nl);
        centrals.extend_from_slice(&[0u8; 12]);
        u32le(&mut centrals, offset);
        centrals.extend_from_slice(name);
        offset = offset.saturating_add(30).saturating_add(nl).saturating_add(bl);
    }
    let cd_size = centrals.len() as u64;
    let n = files.len() as u64;
    let mut out = locals;
    out.extend_from_slice(&centrals);
    u32le(&mut out, 0x0605_4b50);
    u16le(&mut out, 0);
    u16le(&mut out, 0);
    u16le(&mut out, n);
    u16le(&mut out, n);
    u32le(&mut out, cd_size);
    u32le(&mut out, offset);
    u16le(&mut out, 0);
    out
}

/// One entry read back by [`unzip`].
#[derive(Debug, Clone)]
pub struct UnzipEntry {
    pub name: String,
    pub data: Vec<u8>,
    pub crc: u32,
    pub crc_ok: bool,
    pub method: u16,
}

/// `unzip(bytes)` (stored entries; for tests): None when it is not a zip this can read.
pub fn unzip(bytes: &[u8]) -> Option<Vec<UnzipEntry>> {
    let rd16 = |p: usize| -> Option<u16> { Some(u16::from_le_bytes([*bytes.get(p)?, *bytes.get(p.checked_add(1)?)?])) };
    let rd32 = |p: usize| -> Option<u32> {
        Some(u32::from_le_bytes([
            *bytes.get(p)?,
            *bytes.get(p.checked_add(1)?)?,
            *bytes.get(p.checked_add(2)?)?,
            *bytes.get(p.checked_add(3)?)?,
        ]))
    };
    let mut e = bytes.len().checked_sub(22)?;
    loop {
        if rd32(e) == Some(0x0605_4b50) {
            break;
        }
        e = e.checked_sub(1)?;
    }
    let n = rd16(e.checked_add(10)?)?;
    let mut p = usize::try_from(rd32(e.checked_add(16)?)?).ok()?;
    let mut out = Vec::new();
    for _ in 0..n {
        let name_len = usize::from(rd16(p.checked_add(28)?)?);
        let extra = usize::from(rd16(p.checked_add(30)?)?);
        let comment = usize::from(rd16(p.checked_add(32)?)?);
        let local = usize::try_from(rd32(p.checked_add(42)?)?).ok()?;
        let size = usize::try_from(rd32(p.checked_add(20)?)?).ok()?;
        let crc = rd32(p.checked_add(16)?)?;
        let method = rd16(p.checked_add(10)?)?;
        let name_start = p.checked_add(46)?;
        let name = String::from_utf8_lossy(bytes.get(name_start..name_start.checked_add(name_len)?)?).into_owned();
        let lnl = usize::from(rd16(local.checked_add(26)?)?);
        let lex = usize::from(rd16(local.checked_add(28)?)?);
        let ds = local.checked_add(30)?.checked_add(lnl)?.checked_add(lex)?;
        let data = bytes.get(ds..ds.checked_add(size)?)?.to_vec();
        let crc_ok = crc32(&data) == crc;
        out.push(UnzipEntry { name, data, crc, crc_ok, method });
        p = p.checked_add(46)?.checked_add(name_len)?.checked_add(extra)?.checked_add(comment)?;
    }
    Some(out)
}

/// `xml(s)`: XML-illegal characters dropped, the rest escaped.
pub fn xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let u = c as u32;
        if (u < 0x20 && u != 0x09 && u != 0x0a && u != 0x0d) || u == 0xfffe || u == 0xffff {
            continue;
        }
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

// ---- SHA-1 (for the name-based GUIDs) ----

fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0];
    let bit_len = u64::try_from(data.len()).unwrap_or(0).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for (i, word) in chunk.chunks(4).enumerate().take(16) {
            if let (Some(slot), [a, b, c, d]) = (w.get_mut(i), word) {
                *slot = u32::from_be_bytes([*a, *b, *c, *d]);
            }
        }
        for i in 16..80usize {
            let g = |k: usize| w.get(i.wrapping_sub(k)).copied().unwrap_or(0);
            let v = g(3) ^ g(8) ^ g(14) ^ g(16);
            if let Some(slot) = w.get_mut(i) {
                *slot = v.rotate_left(1);
            }
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let tmp = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = tmp;
        }
        let [h0, h1, h2, h3, h4] = h;
        h = [h0.wrapping_add(a), h1.wrapping_add(b), h2.wrapping_add(c), h3.wrapping_add(d), h4.wrapping_add(e)];
    }
    let mut out = [0u8; 20];
    for (dst, v) in out.chunks_mut(4).zip(h.iter()) {
        dst.copy_from_slice(&v.to_be_bytes());
    }
    out
}

/// `guidFor(name)`: UUID v5 (SHA-1, name-based) in MarkupCraft's namespace.
pub fn guid_for(name: &str) -> String {
    const NS: [u8; 16] = [0x6f, 0x1c, 0x5d, 0x3a, 0x92, 0x0e, 0x4b, 0x7d, 0x8a, 0x40, 0x21, 0x5e, 0x9c, 0x33, 0x7b, 0x02];
    let mut data = NS.to_vec();
    data.extend_from_slice(name.as_bytes());
    let full = sha1(&data);
    let mut h = [0u8; 16];
    h.copy_from_slice(full.get(..16).unwrap_or(&[0u8; 16]));
    if let Some(b) = h.get_mut(6) {
        *b = (*b & 0x0f) | 0x50;
    }
    if let Some(b) = h.get_mut(8) {
        *b = (*b & 0x3f) | 0x80;
    }
    let x: String = h.iter().map(|b| format!("{b:02x}")).collect();
    let part = |a: usize, b: usize| x.get(a..b).unwrap_or("");
    format!("{}-{}-{}-{}-{}", part(0, 8), part(8, 12), part(12, 16), part(16, 20), part(20, 32))
}

/// TopicType values (the BCF file's own vocabulary, English always); types without one read as "".
fn topic_type(k: &str) -> Option<&'static str> {
    Some(match k {
        "pen" => "Pen",
        "highlight" => "Highlighter",
        "line" => "Line",
        "arrow" => "Arrow",
        "rect" => "Rectangle",
        "ellipse" => "Ellipse",
        "polyline" => "Polyline",
        "polygon" => "Polygon",
        "cloudrect" => "Cloud",
        "cloud" => "Cloud (freeform)",
        "callout" => "Callout",
        "text" => "Text",
        "note" => "Note",
        "stamp" => "Stamp",
        "measure" => "Length",
        "area" => "Area",
        "count" => "Count",
        "foreign" => "Other",
        _ => return None,
    })
}

/// `extensionsXsd()`.
pub fn extensions_xsd() -> String {
    let enum_of =
        |vals: Vec<String>| vals.iter().map(|v| format!("\t\t\t\t<xs:enumeration value=\"{}\"/>", xml(v))).collect::<Vec<_>>().join("\n");
    let types: Vec<String> = TYPE_IDS.iter().map(|k| topic_type(k).unwrap_or("").to_string()).collect();
    let stats: Vec<String> = STATUSES.iter().map(|s| (*s).to_string()).collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\">\n\t<xs:redefine schemaLocation=\"markup.xsd\">\n\t\t<xs:simpleType name=\"TopicType\">\n\t\t\t<xs:restriction base=\"TopicType\">\n{}\n\t\t\t</xs:restriction>\n\t\t</xs:simpleType>\n\t\t<xs:simpleType name=\"TopicStatus\">\n\t\t\t<xs:restriction base=\"TopicStatus\">\n{}\n\t\t\t</xs:restriction>\n\t\t</xs:simpleType>\n\t</xs:redefine>\n</xs:schema>\n",
        enum_of(types),
        enum_of(stats)
    )
}

/// One topic of an export.
#[derive(Debug, Clone)]
pub struct Topic<'a> {
    pub markup: &'a Markup,
    pub sheet_title: String,
    pub file_name: String,
    pub value: String,
    pub bbox: Option<[f64; 4]>,
    pub png: Option<Vec<u8>>,
}

/// `isoDate(s)`: the time as ISO (seconds), or now when it does not read (a date-time without an offset is local).
fn iso_date(s: &str, now_ms: f64, zone: Zone) -> String {
    let t = parse_date_in(s, zone);
    to_iso_s(if t.is_nan() { now_ms } else { t }).or_else(|| to_iso_s(now_ms)).unwrap_or_default()
}

/// `+x.toFixed(6)` printed.
fn fx6(x: f64) -> String {
    ns(str_to_number(&to_fixed(x, 6)))
}

/// `buildBcf(topics, {project, exported})` → (zip bytes, count). `now_ms` stands in for `new Date()` where a date does
/// not read; `zone` is the local time of the zip dates and of dates written without an offset.
pub fn build_bcf(topics: &[Topic<'_>], project: &str, exported_ms: f64, now_ms: f64, zone: Zone) -> (Vec<u8>, usize) {
    let mut files: Vec<ZipFile> = Vec::new();
    let f = |name: &str, s: String| ZipFile { name: name.to_string(), data: s.into_bytes() };
    files.push(f(
        "bcf.version",
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Version VersionId=\"2.1\">\n\t<DetailedVersion>2.1</DetailedVersion>\n</Version>\n"
            .into(),
    ));
    let pid = guid_for(&format!("project:{project}"));
    files.push(f(
        "project.bcfp",
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ProjectExtension>\n\t<Project ProjectId=\"{pid}\">\n\t\t<Name>{}</Name>\n\t</Project>\n\t<ExtensionSchema>extensions.xsd</ExtensionSchema>\n</ProjectExtension>\n",
            xml(project)
        ),
    ));
    files.push(f("extensions.xsd", extensions_xsd()));
    let exported_iso = to_iso_s(exported_ms).or_else(|| to_iso_s(now_ms)).unwrap_or_default();
    for (index, tp) in topics.iter().enumerate() {
        let m = tp.markup;
        let g = guid_for(&format!("topic:{}", m.id));
        let vg = guid_for(&format!("viewpoint:{}", m.id));
        let ty = topic_type(&m.kind).unwrap_or("Other");
        let type_nm = if is_type(&m.kind) { type_name(&m.kind) } else { type_name("foreign") };
        let what: Vec<String> = [
            m.punch.as_ref().map(|p| format!("#{}", ns(p.n))).unwrap_or_default(),
            if m.subject.is_empty() { type_nm.clone() } else { m.subject.clone() },
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
        let what = what.join(" ");
        let title_full = if tp.sheet_title.is_empty() {
            what.clone()
        } else {
            t("bcf.title.on", &[("what", Fill::S(what.clone())), ("sheet", Fill::from(&tp.sheet_title))])
        };
        let title = crate::js::slice16(&title_full, 250);
        let text_part = if m.kind == "stamp" {
            m.label.clone()
        } else if m.kind == "text" || m.kind == "callout" {
            m.text.clone()
        } else {
            String::new()
        };
        let sheet_desc = if tp.sheet_title.is_empty() {
            String::new()
        } else if !tp.file_name.is_empty() {
            t("bcf.desc.sheet_file", &[("sheet", Fill::from(&tp.sheet_title)), ("file", Fill::from(&tp.file_name))])
        } else {
            t("bcf.desc.sheet", &[("sheet", Fill::from(&tp.sheet_title))])
        };
        let description: Vec<String> =
            [text_part, tp.value.clone(), m.comment.clone(), sheet_desc].into_iter().filter(|s| !s.is_empty()).collect();
        let description = description.join("\n");
        let labels: Vec<String> = [type_nm.clone(), tp.sheet_title.clone()].into_iter().filter(|s| !s.is_empty()).collect();
        let mut lines: Vec<String> = vec!["<?xml version=\"1.0\" encoding=\"UTF-8\"?>".into(), "<Markup>".into()];
        if !tp.file_name.is_empty() {
            lines.push("\t<Header>".into());
            lines.push("\t\t<File isExternal=\"true\">".into());
            lines.push(format!("\t\t\t<Filename>{}</Filename>", xml(&tp.file_name)));
            lines.push(format!("\t\t\t<Date>{exported_iso}</Date>"));
            lines.push("\t\t</File>".into());
            lines.push("\t</Header>".into());
        }
        let status = if STATUSES.contains(&m.status.as_str()) { m.status.as_str() } else { "Open" };
        lines.push(format!("\t<Topic Guid=\"{g}\" TopicType=\"{}\" TopicStatus=\"{}\">", xml(ty), xml(status)));
        lines.push(format!("\t\t<Title>{}</Title>", xml(if title.is_empty() { ty } else { &title })));
        lines.push(format!("\t\t<Index>{}</Index>", index.saturating_add(1)));
        for l in &labels {
            lines.push(format!("\t\t<Labels>{}</Labels>", xml(l)));
        }
        let author = if m.author.is_empty() { "Unknown" } else { m.author.as_str() };
        lines.push(format!("\t\t<CreationDate>{}</CreationDate>", iso_date(&m.created, now_ms, zone)));
        lines.push(format!("\t\t<CreationAuthor>{}</CreationAuthor>", xml(author)));
        if !m.modified.is_empty() && m.modified != m.created {
            lines.push(format!("\t\t<ModifiedDate>{}</ModifiedDate>", iso_date(&m.modified, now_ms, zone)));
            lines.push(format!("\t\t<ModifiedAuthor>{}</ModifiedAuthor>", xml(author)));
        }
        if let Some(p) = &m.punch {
            let b = p.due.as_bytes();
            if b.len() == 10 && b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() }) {
                lines.push(format!("\t\t<DueDate>{}T00:00:00Z</DueDate>", p.due));
            }
            if !p.assignee.is_empty() {
                lines.push(format!("\t\t<AssignedTo>{}</AssignedTo>", xml(&p.assignee)));
            }
        }
        if !description.is_empty() {
            lines.push(format!("\t\t<Description>{}</Description>", xml(&description)));
        }
        lines.push("\t</Topic>".into());
        for r in &m.replies {
            let cg = guid_for(&format!("comment:{}:{}", m.id, r.id));
            lines.push(format!("\t<Comment Guid=\"{cg}\">"));
            lines.push(format!("\t\t<Date>{}</Date>", iso_date(&r.date, now_ms, zone)));
            lines.push(format!("\t\t<Author>{}</Author>", xml(if r.author.is_empty() { "Unknown" } else { &r.author })));
            lines.push(format!("\t\t<Comment>{}</Comment>", xml(&r.text)));
            lines.push(format!("\t\t<Viewpoint Guid=\"{vg}\"/>"));
            lines.push("\t</Comment>".into());
        }
        lines.push(format!("\t<Viewpoints Guid=\"{vg}\">"));
        lines.push("\t\t<Viewpoint>viewpoint.bcfv</Viewpoint>".into());
        if tp.png.is_some() {
            lines.push("\t\t<Snapshot>snapshot.png</Snapshot>".into());
        }
        lines.push("\t\t<Index>0</Index>".into());
        lines.push("\t</Viewpoints>".into());
        lines.push("</Markup>".into());
        files.push(f(&format!("{g}/markup.bcf"), format!("{}\n", lines.join("\n"))));
        let b = tp.bbox.or(m.rect).unwrap_or([0.0; 4]);
        let k = 0.0254 / 72.0;
        let cx = ((b[0] + b[2]) / 2.0) * k;
        let cy = ((b[1] + b[3]) / 2.0) * k;
        let span = crate::js::max(crate::js::max(b[2] - b[0], b[3] - b[1]), 36.0) * k;
        let pt = |x: f64, y: f64, z: f64| format!("<X>{}</X><Y>{}</Y><Z>{}</Z>", fx6(x), fx6(y), fx6(z));
        files.push(f(
            &format!("{g}/viewpoint.bcfv"),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<VisualizationInfo Guid=\"{vg}\">\n\t<OrthogonalCamera>\n\t\t<CameraViewPoint>{}</CameraViewPoint>\n\t\t<CameraDirection>{}</CameraDirection>\n\t\t<CameraUpVector>{}</CameraUpVector>\n\t\t<ViewToWorldScale>{}</ViewToWorldScale>\n\t</OrthogonalCamera>\n</VisualizationInfo>\n",
                pt(cx, cy, 10.0),
                pt(0.0, 0.0, -1.0),
                pt(0.0, 1.0, 0.0),
                fx6(span * 1.4)
            ),
        ));
        if let Some(png) = &tp.png {
            files.push(ZipFile { name: format!("{g}/snapshot.png"), data: png.clone() });
        }
    }
    (zip(&files, exported_ms, zone), topics.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sha_and_guid() {
        let h: String = sha1(b"abc").iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(h, "a9993e364706816aba3e25717850c26c9cd0d89d");
        let g = guid_for("topic:x");
        assert_eq!(g.len(), 36);
        assert_eq!(&g[14..15], "5");
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }

    /// The DOS time / date of a zip entry (local header bytes 10..14).
    fn dos(z: &[u8]) -> (u16, u16) {
        (u16::from_le_bytes([z[10], z[11]]), u16::from_le_bytes([z[12], z[13]]))
    }

    #[test]
    fn zip_dates_are_local_time() {
        // bcf.js zip(files, new Date(1791810123000)) under TZ=Asia/Kolkata, Etc/GMT+6, Pacific/Chatham, UTC
        let t = 1_791_806_400_000.0 + 3_723_000.0;
        let f = [ZipFile { name: "a".into(), data: b"x".to_vec() }];
        assert_eq!(dos(&zip(&f, t, Zone::Fixed(330))), (0x9401, 0x5d4c));
        assert_eq!(dos(&zip(&f, t, Zone::Fixed(-360))), (0x3841, 0x5d4c));
        assert_eq!(dos(&zip(&f, t, Zone::Fixed(825))), (0x15e1, 0x5d4d));
        assert_eq!(dos(&zip(&f, t, Zone::Utc)), (0x6841, 0x5d4c));
        // 1980-01-01T05:59:58Z is still 1979 in Denver: the year wraps as the JS's 32-bit shift does (not clamped)
        assert_eq!(dos(&zip(&f, 315_554_398_000.0, Zone::Fixed(-360))), (0xbf7d, 0xff9f));
        // the workbook's created date stays UTC; its zip dates are local
        let x = crate::xlsx::write_xlsx(&[], "T", false, t, Zone::Fixed(-360));
        assert_eq!(dos(&x), (0x3841, 0x5d4c));
        assert!(
            String::from_utf8_lossy(&x).contains("<dcterms:created xsi:type=\"dcterms:W3CDTF\">2026-10-12T13:02:03Z</dcterms:created>")
        );
    }
}
