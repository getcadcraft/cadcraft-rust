//! xlsx.js — a minimal Office Open XML workbook writer (MarkupCraft's copy of TakeoffCraft's, 0.4 K6).
//!
//! `writeXlsx(sheets, {title, compress})`; sheets and cells are the JS objects (JSON values here):
//! `{name, cols: [widths], rows: [[cell…]…], freeze}`, cell = null | text | number | `{v, f, s}`. Strings are inline,
//! formulas carry their value, styles are the `STYLE` keys. With `compress: false` the bytes equal the JS's; with
//! `compress: true` the entries are deflated by miniz instead of zlib (same contents).

use crate::bcf::{ZipFile, zip_with};
use crate::js::{num_to_string, to_js_string, trim};
use serde_json::Value;

/// `STYLE`: style key → cellXfs index.
pub const STYLE: [(&str, u32); 10] =
    [("none", 0), ("head", 1), ("qty", 2), ("pct", 3), ("rate", 4), ("money", 5), ("bold", 6), ("boldMoney", 7), ("text", 8), ("int", 9)];

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

/// `xmlText(s)`: characters XML 1.0 forbids dropped, `& < > "` escaped.
pub fn xml_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let u = u32::from(c);
        let ok = u == 9 || u == 10 || u == 13 || (0x20..=0xd7ff).contains(&u) || (0xe000..=0xfffd).contains(&u) || u >= 0x10000;
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

/// `colName(i)` (0 → A).
pub fn col_name(i: usize) -> String {
    let mut s = Vec::new();
    let mut i = i.saturating_add(1);
    while i > 0 {
        let m = (i.saturating_sub(1)) % 26;
        s.push(char::from(b'A'.saturating_add(u8::try_from(m).unwrap_or(0))));
        i = i.saturating_sub(1) / 26;
    }
    s.iter().rev().collect()
}

/// `ref(r, c)`.
pub fn cell_ref(r: usize, c: usize) -> String {
    format!("{}{}", col_name(c), r.saturating_add(1))
}

/// `sheetName(name, used)`.
pub fn sheet_name(name: &str, used: &mut Vec<String>) -> String {
    let a: String = name.chars().map(|c| if matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\') { ' ' } else { c }).collect();
    let b = a.trim_start_matches('\'').trim_end_matches('\'');
    let base0 = crate::js::slice16(trim(b), 31);
    let base = if base0.is_empty() { "Sheet".to_string() } else { base0 };
    let mut n = base.clone();
    let mut k = 2u64;
    while used.contains(&n.to_lowercase()) {
        let ks = k.to_string();
        let keep = 31usize.saturating_sub(ks.len()).saturating_sub(1);
        n = format!("{} {ks}", crate::js::slice16(&base, keep));
        k = k.saturating_add(1);
    }
    used.push(n.to_lowercase());
    n
}

fn num_str(n: f64) -> String {
    if n == 0.0 { "0".into() } else { num_to_string(n) }
}

fn js_string(v: Option<&Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(x) => to_js_string(x),
    }
}

fn style_index(s: Option<&Value>) -> u32 {
    s.and_then(Value::as_str).and_then(|k| STYLE.iter().find(|(n, _)| *n == k)).map_or(0, |(_, i)| *i)
}

/// `cellXml(v, r, c)`.
pub fn cell_xml(v: &Value, r: usize, c: usize) -> String {
    match v {
        Value::Null => return String::new(),
        Value::String(s) if s.is_empty() => return String::new(),
        _ => {}
    }
    let (cv, f, s): (Option<&Value>, Option<&Value>, Option<&Value>) = match v {
        Value::Object(o) => (o.get("v"), o.get("f"), o.get("s")),
        Value::Array(_) => (None, None, None),
        x => (Some(x), None, None),
    };
    let si = style_index(s);
    let at = format!(" r=\"{}\"{}", cell_ref(r, c), if si != 0 { format!(" s=\"{si}\"") } else { String::new() });
    if crate::js::truthy(f) {
        let val = match cv.and_then(Value::as_f64) {
            Some(x) if x.is_finite() && cv.is_some_and(Value::is_number) => format!("<v>{}</v>", num_str(x)),
            _ => String::new(),
        };
        return format!("<c{at}><f>{}</f>{val}</c>", xml_text(&js_string(f)));
    }
    if let Some(Value::Number(n)) = cv {
        let x = n.as_f64().unwrap_or(f64::NAN);
        return if x.is_finite() { format!("<c{at}><v>{}</v></c>", num_str(x)) } else { String::new() };
    }
    format!("<c{at} t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>", xml_text(&js_string(cv)))
}

/// `sheetXml(sh)`.
pub fn sheet_xml(sh: &Value) -> String {
    let cols: String = sh
        .get("cols")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .enumerate()
                .map(|(i, w)| {
                    format!(
                        "<col min=\"{}\" max=\"{}\" width=\"{}\" customWidth=\"1\"/>",
                        i.saturating_add(1),
                        i.saturating_add(1),
                        to_js_string(w)
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let rows: String = sh
        .get("rows")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .enumerate()
                .map(|(r, row)| {
                    let cells: String =
                        row.as_array().map(|cs| cs.iter().enumerate().map(|(c, v)| cell_xml(v, r, c)).collect()).unwrap_or_default();
                    format!("<row r=\"{}\">{cells}</row>", r.saturating_add(1))
                })
                .collect()
        })
        .unwrap_or_default();
    let fz = sh.get("freeze");
    let freeze = if crate::js::truthy(fz) {
        let f = fz.cloned().unwrap_or(Value::Null);
        let next = match &f {
            Value::Number(n) => num_to_string(n.as_f64().unwrap_or(f64::NAN) + 1.0),
            x => format!("{}1", to_js_string(x)),
        };
        format!(
            "<sheetViews><sheetView workbookViewId=\"0\"><pane ySplit=\"{}\" topLeftCell=\"A{next}\" activePane=\"bottomLeft\" state=\"frozen\"/></sheetView></sheetViews>",
            to_js_string(&f)
        )
    } else {
        String::new()
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">{freeze}{}<sheetData>{rows}</sheetData></worksheet>",
        if cols.is_empty() { String::new() } else { format!("<cols>{cols}</cols>") }
    )
}

/// `writeXlsx(sheets, {title, compress})` at time `now_ms` (the core properties' date and the zip's).
pub fn write_xlsx(sheets: &[Value], title: &str, compress: bool, now_ms: f64, zone: cc_clock::Zone) -> Vec<u8> {
    let mut used = Vec::new();
    let named: Vec<(String, &Value)> = sheets
        .iter()
        .map(|s| {
            let nm = match s.get("name") {
                None => "undefined".to_string(),
                Some(v) => to_js_string(v),
            };
            (sheet_name(&nm, &mut used), s)
        })
        .collect();
    let mut ct = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/><Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/>",
    );
    for i in 0..named.len() {
        ct.push_str(&format!(
            "<Override PartName=\"/xl/worksheets/sheet{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>",
            i.saturating_add(1)
        ));
    }
    ct.push_str("<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/><Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/></Types>");
    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/><Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties\" Target=\"docProps/app.xml\"/></Relationships>";
    let created = crate::js::to_iso_s(now_ms).unwrap_or_default();
    let core = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dc:title>{}</dc:title><dc:creator>TakeoffCraft</dc:creator><dcterms:created xsi:type=\"dcterms:W3CDTF\">{created}</dcterms:created></cp:coreProperties>",
        xml_text(title)
    );
    let app = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>TakeoffCraft</Application></Properties>";
    let mut wb = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><bookViews><workbookView/></bookViews><sheets>",
    );
    for (i, (n, _)) in named.iter().enumerate() {
        let k = i.saturating_add(1);
        wb.push_str(&format!("<sheet name=\"{}\" sheetId=\"{k}\" r:id=\"rId{k}\"/>", xml_text(n)));
    }
    wb.push_str("</sheets><calcPr calcId=\"191029\" fullCalcOnLoad=\"1\"/></workbook>");
    let mut wr = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
    );
    for i in 0..named.len() {
        let k = i.saturating_add(1);
        wr.push_str(&format!(
            "<Relationship Id=\"rId{k}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet{k}.xml\"/>"
        ));
    }
    wr.push_str(&format!(
        "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/></Relationships>",
        named.len().saturating_add(1)
    ));
    let mut files = vec![
        ZipFile { name: "[Content_Types].xml".into(), data: ct.into_bytes() },
        ZipFile { name: "_rels/.rels".into(), data: rels.as_bytes().to_vec() },
        ZipFile { name: "docProps/core.xml".into(), data: core.into_bytes() },
        ZipFile { name: "docProps/app.xml".into(), data: app.as_bytes().to_vec() },
        ZipFile { name: "xl/workbook.xml".into(), data: wb.into_bytes() },
        ZipFile { name: "xl/_rels/workbook.xml.rels".into(), data: wr.into_bytes() },
        ZipFile { name: "xl/styles.xml".into(), data: STYLES_XML.as_bytes().to_vec() },
    ];
    for (i, (_, s)) in named.iter().enumerate() {
        files.push(ZipFile { name: format!("xl/worksheets/sheet{}.xml", i.saturating_add(1)), data: sheet_xml(s).into_bytes() });
    }
    zip_with(&files, now_ms, zone, compress)
}
