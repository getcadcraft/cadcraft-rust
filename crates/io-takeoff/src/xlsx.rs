//! `xlsx.js` — a minimal Office Open XML workbook writer and the zip it lives in: inline strings, formulas with their
//! cached values, 10 styles, frozen header rows, sanitized unique sheet names, XML-illegal characters dropped.
//!
//! Stored (`compress: false`) workbooks are byte-identical to the JS's for the same date. Deflated ones use
//! miniz_oxide where the JS uses the runtime's CompressionStream (Node: Chromium's zlib): the entries are the same
//! bytes after inflating, the deflate streams differ (Spec gap: zlib's output depends on the zlib build).

use cc_takeoff::jv;

// evaluated at compile time: an out-of-range index or overflow would be a build error, not a runtime panic
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
const CRC_TABLE: [u32; 256] = {
    let mut t = [0u32; 256];
    let mut n = 0u32;
    while n < 256 {
        let mut c = n;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[n as usize] = c;
        n += 1;
    }
    t
};

/// CRC-32 (zip).
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for b in bytes {
        let i = usize::from(((c ^ u32::from(*b)) & 0xFF) as u8);
        c = CRC_TABLE.get(i).copied().unwrap_or(0) ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

/// A local date and time for the zip entries and the workbook's created date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub min: u32,
    pub sec: u32,
    pub ms: u32,
}
impl Stamp {
    /// From a time in milliseconds since 1970 (UTC fields; the JS uses local time: the caller passes local).
    // days are clamped to ±1e8 before the civil-date arithmetic, so none of it can overflow
    #[allow(clippy::arithmetic_side_effects)]
    pub fn from_ms(ms: f64) -> Stamp {
        let total_ms = if ms.is_finite() { ms } else { 0.0 };
        let days = (total_ms / 86_400_000.0).floor();
        let rem = total_ms - days * 86_400_000.0;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let rem_ms = rem.clamp(0.0, 86_399_999.0) as u64;
        #[allow(clippy::cast_possible_truncation)]
        let z = days.clamp(-1e8, 1e8) as i64 + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };
        Stamp {
            year: i32::try_from(y).unwrap_or(1980),
            month: u32::try_from(m).unwrap_or(1),
            day: u32::try_from(d).unwrap_or(1),
            hour: u32::try_from(rem_ms / 3_600_000).unwrap_or(0),
            min: u32::try_from(rem_ms / 60_000 % 60).unwrap_or(0),
            sec: u32::try_from(rem_ms / 1000 % 60).unwrap_or(0),
            ms: u32::try_from(rem_ms % 1000).unwrap_or(0),
        }
    }
    /// `toISOString()` (`YYYY-MM-DDTHH:MM:SS.mmmZ`).
    pub fn iso(&self) -> String {
        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z", self.year, self.month, self.day, self.hour, self.min, self.sec, self.ms)
    }
}

fn deflate_raw(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec(data, 6)
}

fn u16le(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&((v & 0xFFFF) as u16).to_le_bytes());
}
fn u32le(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&((v & 0xFFFF_FFFF) as u32).to_le_bytes());
}

/// files: (name, data) → zip bytes.
pub fn zip(files: &[(String, Vec<u8>)], compress: bool, date: &Stamp) -> Vec<u8> {
    let dos_time = u64::from(((date.hour << 11) | (date.min << 5) | (date.sec >> 1)) & 0xFFFF);
    let yr = i64::from(date.year).saturating_sub(1980);
    // the JS shifts a 32-bit int and keeps the low 16 bits: years outside 1980–2107 wrap the same way here
    let dos_date = (u64::try_from(yr.wrapping_shl(9) & 0xFFFF).unwrap_or(0) | u64::from(date.month << 5) | u64::from(date.day)) & 0xFFFF;
    let mut out = Vec::new();
    let mut central = Vec::new();
    let mut offset: u64 = 0;
    for (name, data) in files {
        let nb = name.as_bytes();
        let crc = u64::from(crc32(data));
        let mut body: &[u8] = data;
        let mut method = 0u64;
        let deflated;
        if compress {
            deflated = deflate_raw(data);
            if deflated.len() < data.len() {
                body = &deflated;
                method = 8;
            }
        }
        let (bl, dl, nl) = (body.len() as u64, data.len() as u64, nb.len() as u64);
        u32le(&mut out, 0x0403_4b50);
        u16le(&mut out, 20);
        u16le(&mut out, 0x0800);
        u16le(&mut out, method);
        u16le(&mut out, dos_time);
        u16le(&mut out, dos_date);
        u32le(&mut out, crc);
        u32le(&mut out, bl);
        u32le(&mut out, dl);
        u16le(&mut out, nl);
        u16le(&mut out, 0);
        out.extend_from_slice(nb);
        out.extend_from_slice(body);
        u32le(&mut central, 0x0201_4b50);
        u16le(&mut central, 20);
        u16le(&mut central, 20);
        u16le(&mut central, 0x0800);
        u16le(&mut central, method);
        u16le(&mut central, dos_time);
        u16le(&mut central, dos_date);
        u32le(&mut central, crc);
        u32le(&mut central, bl);
        u32le(&mut central, dl);
        u16le(&mut central, nl);
        // extra, comment, disk, internal attributes (zeros), external attributes (zero)
        central.extend_from_slice(&[0u8; 12]);
        u32le(&mut central, offset);
        central.extend_from_slice(nb);
        offset = offset.saturating_add(30).saturating_add(nl).saturating_add(bl);
    }
    let cd_size = central.len() as u64;
    out.extend_from_slice(&central);
    u32le(&mut out, 0x0605_4b50);
    u16le(&mut out, 0);
    u16le(&mut out, 0);
    u16le(&mut out, files.len() as u64);
    u16le(&mut out, files.len() as u64);
    u32le(&mut out, cd_size);
    u32le(&mut out, offset);
    u16le(&mut out, 0);
    out
}

/// Style keys → cellXfs index.
pub fn style_index(s: &str) -> u32 {
    match s {
        "head" => 1,
        "qty" => 2,
        "pct" => 3,
        "rate" => 4,
        "money" => 5,
        "bold" => 6,
        "boldMoney" => 7,
        "text" => 8,
        "int" => 9,
        _ => 0,
    }
}

const STYLES_XML: &str = r##"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<numFmts count="4"><numFmt numFmtId="164" formatCode="#,##0.00"/><numFmt numFmtId="165" formatCode="0.0#%"/><numFmt numFmtId="166" formatCode="&quot;$&quot;#,##0.00##"/><numFmt numFmtId="167" formatCode="&quot;$&quot;#,##0.00"/></numFmts>
<fonts count="2"><font><sz val="11"/><name val="Calibri"/><family val="2"/></font><font><b/><sz val="11"/><name val="Calibri"/><family val="2"/></font></fonts>
<fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills>
<borders count="2"><border><left/><right/><top/><bottom/><diagonal/></border><border><left/><right/><top/><bottom style="thin"><color auto="1"/></bottom><diagonal/></border></borders>
<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>
<cellXfs count="10">
<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>
<xf numFmtId="0" fontId="1" fillId="0" borderId="1" xfId="0" applyFont="1" applyBorder="1"/>
<xf numFmtId="164" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>
<xf numFmtId="165" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>
<xf numFmtId="166" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>
<xf numFmtId="167" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>
<xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/>
<xf numFmtId="167" fontId="1" fillId="0" borderId="0" xfId="0" applyNumberFormat="1" applyFont="1"/>
<xf numFmtId="49" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>
<xf numFmtId="1" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>
</cellXfs>
<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles>
</styleSheet>"##;

/// XML text: drop characters XML 1.0 forbids, escape & < > ".
pub fn xml_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let u = c as u32;
        let ok = matches!(u, 0x09 | 0x0A | 0x0D) || (0x20..=0xD7FF).contains(&u) || (0xE000..=0xFFFD).contains(&u) || u >= 0x10000;
        if !ok {
            continue;
        }
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

/// Column letters of a 0-based index.
pub fn col_name(i: usize) -> String {
    let mut s = Vec::new();
    let mut i = i.saturating_add(1);
    while i > 0 {
        let i1 = i.saturating_sub(1);
        s.push(b'A'.saturating_add(u8::try_from(i1 % 26).unwrap_or(0)));
        i = i1 / 26;
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}
/// `ref(r, c)`: "A1" of a 0-based row and column.
pub fn cref(r: usize, c: usize) -> String {
    format!("{}{}", col_name(c), r.saturating_add(1))
}

/// Sheet names: ≤ 31 chars, none of []:*?/\ , unique (case-insensitive).
pub fn sheet_name(name: &str, used: &mut Vec<String>) -> String {
    let rep: String = name.chars().map(|c| if matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\') { ' ' } else { c }).collect();
    let trimmed = rep.trim_start_matches('\'').trim_end_matches('\'');
    let base0 = cc_m3d::js::slice16(cc_m3d::js::trim(trimmed), 31);
    let base = if base0.is_empty() { "Sheet".to_string() } else { base0 };
    let mut nm = base.clone();
    let mut k = 2usize;
    while used.contains(&jv::lower(&nm)) {
        let ks = k.to_string();
        nm = format!("{} {k}", cc_m3d::js::slice16(&base, 31usize.saturating_sub(ks.len()).saturating_sub(1)));
        k = k.saturating_add(1);
    }
    used.push(jv::lower(&nm));
    nm
}

/// A cell's value.
#[derive(Debug, Clone, PartialEq)]
pub enum CellV {
    Undefined,
    Num(f64),
    Str(String),
}
/// A cell: null / '' (empty), a number, text, or `{ v, f, s }`.
#[derive(Debug, Clone, PartialEq)]
pub enum XCell {
    Empty,
    Num(f64),
    Str(String),
    Obj { v: CellV, f: Option<String>, s: Option<&'static str> },
}
impl XCell {
    pub fn t(s: impl Into<String>) -> XCell {
        let s = s.into();
        if s.is_empty() { XCell::Empty } else { XCell::Str(s) }
    }
    pub fn vs(v: f64, s: &'static str) -> XCell {
        XCell::Obj { v: CellV::Num(v), f: None, s: Some(s) }
    }
    pub fn ts(v: impl Into<String>, s: &'static str) -> XCell {
        XCell::Obj { v: CellV::Str(v.into()), f: None, s: Some(s) }
    }
    pub fn tv(v: impl Into<String>) -> XCell {
        XCell::Obj { v: CellV::Str(v.into()), f: None, s: None }
    }
    pub fn fs(v: f64, f: impl Into<String>, s: &'static str) -> XCell {
        XCell::Obj { v: CellV::Num(v), f: Some(f.into()), s: Some(s) }
    }
    /// The cell as the JS object (for the parity harness).
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::{Map, Value};
        match self {
            XCell::Empty => Value::Null,
            XCell::Num(x) => jv::n(*x),
            XCell::Str(s) => Value::String(s.clone()),
            XCell::Obj { v, f, s } => {
                let mut o = Map::new();
                match v {
                    CellV::Undefined => {}
                    CellV::Num(x) => {
                        o.insert("v".into(), jv::n(*x));
                    }
                    CellV::Str(t) => {
                        o.insert("v".into(), Value::String(t.clone()));
                    }
                }
                if let Some(f) = f {
                    o.insert("f".into(), Value::String(f.clone()));
                }
                if let Some(s) = s {
                    o.insert("s".into(), Value::String((*s).into()));
                }
                Value::Object(o)
            }
        }
    }
}

fn num_str(n: f64) -> String {
    if n == 0.0 { "0".into() } else { cc_takeoff::jv::num_str(n) }
}

fn cell_xml(out: &mut String, v: &XCell, r: usize, c: usize) {
    let (val, f, s) = match v {
        XCell::Empty => return,
        XCell::Num(x) => (CellV::Num(*x), None, None),
        XCell::Str(t) => {
            if t.is_empty() {
                return;
            }
            (CellV::Str(t.clone()), None, None)
        }
        XCell::Obj { v, f, s } => (v.clone(), f.clone(), *s),
    };
    let si = s.map_or(0, style_index);
    let mut at = format!(" r=\"{}\"", cref(r, c));
    if si != 0 {
        at.push_str(&format!(" s=\"{si}\""));
    }
    if let Some(f) = f.filter(|f| !f.is_empty()) {
        let vx = match val {
            CellV::Num(x) if x.is_finite() => format!("<v>{}</v>", num_str(x)),
            _ => String::new(),
        };
        out.push_str(&format!("<c{at}><f>{}</f>{vx}</c>", xml_text(&f)));
        return;
    }
    match val {
        CellV::Num(x) => {
            if x.is_finite() {
                out.push_str(&format!("<c{at}><v>{}</v></c>", num_str(x)));
            }
        }
        CellV::Str(t) => out.push_str(&format!("<c{at} t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>", xml_text(&t))),
        CellV::Undefined => out.push_str(&format!("<c{at} t=\"inlineStr\"><is><t xml:space=\"preserve\">undefined</t></is></c>")),
    }
}

/// A worksheet: name, rows, column widths, frozen header rows.
#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    pub name: String,
    pub rows: Vec<Vec<XCell>>,
    pub cols: Vec<f64>,
    pub freeze: usize,
}

fn sheet_xml(sh: &Sheet) -> String {
    let mut cols = String::new();
    for (i, w) in sh.cols.iter().enumerate() {
        cols.push_str(&format!(
            "<col min=\"{}\" max=\"{}\" width=\"{}\" customWidth=\"1\"/>",
            i.saturating_add(1),
            i.saturating_add(1),
            cc_takeoff::jv::num_str(*w)
        ));
    }
    let mut rows = String::new();
    for (r, row) in sh.rows.iter().enumerate() {
        rows.push_str(&format!("<row r=\"{}\">", r.saturating_add(1)));
        for (c, v) in row.iter().enumerate() {
            cell_xml(&mut rows, v, r, c);
        }
        rows.push_str("</row>");
    }
    let freeze = if sh.freeze > 0 {
        format!(
            "<sheetViews><sheetView workbookViewId=\"0\"><pane ySplit=\"{}\" topLeftCell=\"A{}\" activePane=\"bottomLeft\" state=\"frozen\"/></sheetView></sheetViews>",
            sh.freeze,
            sh.freeze.saturating_add(1)
        )
    } else {
        String::new()
    };
    let cols = if cols.is_empty() { String::new() } else { format!("<cols>{cols}</cols>") };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">{freeze}{cols}<sheetData>{rows}</sheetData></worksheet>"
    )
}

/// The files of a workbook (before zipping).
pub fn workbook_files(sheets: &[Sheet], title: &str, created: &Stamp) -> Vec<(String, Vec<u8>)> {
    let mut used = Vec::new();
    let names: Vec<String> = sheets.iter().map(|s| sheet_name(&s.name, &mut used)).collect();
    let mut ct = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/><Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/>",
    );
    for i in 0..names.len() {
        ct.push_str(&format!("<Override PartName=\"/xl/worksheets/sheet{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>", i.saturating_add(1)));
    }
    ct.push_str("<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/><Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/></Types>");
    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/><Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties\" Target=\"docProps/app.xml\"/></Relationships>".to_string();
    let iso = created.iso();
    let created_s = iso.rsplit_once('.').map_or(iso.clone(), |(a, _)| format!("{a}Z"));
    let core = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dc:title>{}</dc:title><dc:creator>TakeoffCraft</dc:creator><dcterms:created xsi:type=\"dcterms:W3CDTF\">{created_s}</dcterms:created></cp:coreProperties>",
        xml_text(title)
    );
    let app = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>TakeoffCraft</Application></Properties>".to_string();
    let mut wb = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><bookViews><workbookView/></bookViews><sheets>",
    );
    for (i, nm) in names.iter().enumerate() {
        wb.push_str(&format!("<sheet name=\"{}\" sheetId=\"{}\" r:id=\"rId{}\"/>", xml_text(nm), i.saturating_add(1), i.saturating_add(1)));
    }
    wb.push_str("</sheets><calcPr calcId=\"191029\" fullCalcOnLoad=\"1\"/></workbook>");
    let mut wrels = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
    );
    for i in 0..names.len() {
        wrels.push_str(&format!("<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet{}.xml\"/>", i.saturating_add(1), i.saturating_add(1)));
    }
    wrels.push_str(&format!("<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/></Relationships>", names.len().saturating_add(1)));
    let mut files: Vec<(String, Vec<u8>)> = vec![
        ("[Content_Types].xml".into(), ct.into_bytes()),
        ("_rels/.rels".into(), rels.into_bytes()),
        ("docProps/core.xml".into(), core.into_bytes()),
        ("docProps/app.xml".into(), app.into_bytes()),
        ("xl/workbook.xml".into(), wb.into_bytes()),
        ("xl/_rels/workbook.xml.rels".into(), wrels.into_bytes()),
        ("xl/styles.xml".into(), STYLES_XML.as_bytes().to_vec()),
    ];
    for (i, s) in sheets.iter().enumerate() {
        files.push((format!("xl/worksheets/sheet{}.xml", i.saturating_add(1)), sheet_xml(s).into_bytes()));
    }
    files
}

/// sheets → .xlsx bytes.
pub fn write_xlsx(sheets: &[Sheet], title: &str, compress: bool, date: &Stamp) -> Vec<u8> {
    zip(&workbook_files(sheets, title, date), compress, date)
}
