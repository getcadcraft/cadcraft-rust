//! dwg-equipment.js: the equipment list hidden in a foodservice drawing's item tags (KCL-ITEM and friends), with
//! libitem.js's KCL field decoders (`decodeElec`, `elecText`, `plumbSize`) and the CSV for a spreadsheet.
//!
//! A row is one item number with one set of data; its quantity is the sum of its tags' quantities; the same number on
//! different equipment gives two rows, both marked `clash`. "Sheets" are worked out from each layout's viewports.

use std::collections::HashMap;

use cc_cad::js;
use cc_cad_geom::jsmath::JsMath;
use cc_drawing::{Drawing, SpaceKind};
use cc_io_cad::jsv;
use cc_io_dwg::value::{Obj, V};

use crate::tr;

const TAG: &str = "KCL-ITEM";
/// At most this many tagged references are read (a drawing has a few hundred; the cap keeps a hostile file bounded).
const MAX_TAGS: usize = 200_000;

/// One electrical connection (decodeElec).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Elec {
    pub amps: String,
    pub kw: String,
    pub hp: String,
    pub volts: String,
    pub phase: String,
    pub hz: String,
    /// "direct" | "cord and plug" | ""
    pub conn: &'static str,
    pub nema: String,
}

/// A place a tag sits (model space) and the sheets that show it.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub x: f64,
    pub y: f64,
    pub qty: String,
    pub sheets: Vec<String>,
}

/// One schedule row.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub no: String,
    pub qty: String,
    pub qtys: Vec<String>,
    pub category: String,
    pub mfr: String,
    pub model: String,
    pub remark: String,
    pub elec: Vec<Elec>,
    pub elec_text: String,
    pub plumb_text: String,
    pub el_remark: String,
    pub pl_remark: String,
    pub places: Vec<Place>,
    pub sheets: Vec<String>,
    pub clash: bool,
}

/// A sheet that shows tagged equipment.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetCount {
    pub key: String,
    pub name: String,
    pub count: usize,
}

/// `equipmentList(db, drawing)`.
#[derive(Clone, Debug, PartialEq)]
pub struct EquipList {
    pub rows: Vec<Row>,
    pub tags: usize,
    pub sheets: Vec<SheetCount>,
    pub off_sheet: usize,
}

/// `clean(s)`: `String(s ?? '')` without \p{C} (control, format, private use, surrogates, unassigned), trimmed.
pub fn clean_str(s: &str) -> String {
    use unicode_general_category::{GeneralCategory as G, get_general_category};
    // printable ASCII only (nearly every tag): nothing to drop
    if s.bytes().all(|b| (0x20..0x7f).contains(&b)) {
        return js::trim(s).to_string();
    }
    let kept: String = s
        .chars()
        .filter(|c| !matches!(get_general_category(*c), G::Control | G::Format | G::PrivateUse | G::Surrogate | G::Unassigned))
        .collect();
    js::trim(&kept).to_string()
}
fn clean(v: Option<&V>) -> String {
    clean_str(&jsv::str_nn(v))
}
/// `joinText(...xs)`: cleaned, empty ones dropped, repeats dropped, joined by a space.
pub fn join_text(xs: &[&str]) -> String {
    let mut out: Vec<String> = Vec::new();
    for x in xs {
        let c = clean_str(x);
        if !c.is_empty() && !out.contains(&c) {
            out.push(c);
        }
    }
    out.join(" ")
}

/// libitem.js `decodeElec(s)`: "45~15~~208~3~60~X~~~…" (connections joined by ^) → connections.
pub fn decode_elec(s: &str) -> Vec<Elec> {
    if s.is_empty() {
        return Vec::new();
    }
    s.split('^')
        .map(|c| {
            let f: Vec<&str> = c.split('~').map(js::trim).collect();
            let g = |i: usize| f.get(i).copied().unwrap_or("").to_string();
            let conn = if !g(6).is_empty() {
                "direct"
            } else if !g(7).is_empty() {
                "cord and plug"
            } else {
                ""
            };
            Elec { amps: g(0), kw: g(1), hp: g(2), volts: g(3), phase: g(4), hz: g(5), conn, nema: g(8) }
        })
        .filter(|c| [&c.amps, &c.kw, &c.hp, &c.volts, &c.phase, &c.hz, &c.nema].iter().any(|x| !x.is_empty()) || !c.conn.is_empty())
        .collect()
}

/// libitem.js `elecText(c)`.
pub fn elec_text(c: &Elec) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !c.volts.is_empty() {
        parts.push(format!("{} V{}", c.volts, if c.phase.is_empty() { String::new() } else { format!(" {} ph", c.phase) }));
    }
    if !c.amps.is_empty() {
        parts.push(format!("{} A", c.amps));
    }
    if !c.kw.is_empty() {
        parts.push(format!("{} kW", c.kw));
    }
    if !c.hp.is_empty() {
        parts.push(format!("{} HP", c.hp));
    }
    if !c.conn.is_empty() {
        parts.push(if c.conn == "direct" {
            tr("libitem.elec.direct", &[])
        } else if !c.nema.is_empty() {
            tr("libitem.elec.plug_nema", &[("nema", c.nema.clone())])
        } else {
            tr("libitem.elec.plug", &[])
        });
    }
    parts.join(", ")
}

/// libitem.js `inches(v)`: 0.5 → `1/2"`, 1.5 → `1 1/2"`.
fn inches(v: &str) -> String {
    let n = js::str_to_number(v);
    if !n.is_finite() || n <= 0.0 {
        return v.to_string();
    }
    let whole = n.floor();
    let eighths = js::round((n - whole) * 8.0);
    if eighths == 8.0 {
        return format!("{}\"", js::num_to_string(whole + 1.0));
    }
    let names = ["", "1/8", "1/4", "3/8", "1/2", "5/8", "3/4", "7/8"];
    let f = if eighths != 0.0 { names.get(eighths as usize).copied().unwrap_or("") } else { "" };
    let w = if whole != 0.0 { format!("{}{}", js::num_to_string(whole), if f.is_empty() { "" } else { " " }) } else { String::new() };
    format!("{w}{f}\"")
}

/// libitem.js `plumbSize(s)`: "0.5~39~~" (several joined by ^) → `1/2"` or `(2) 1 1/2"`.
pub fn plumb_size(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let sizes: Vec<&str> = s.split('^').map(|c| js::trim(c.split('~').next().unwrap_or(""))).filter(|z| !z.is_empty()).collect();
    if sizes.is_empty() {
        return String::new();
    }
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for z in sizes {
        match counts.iter_mut().find(|(k, _)| *k == z) {
            Some((_, n)) => *n = n.saturating_add(1),
            None => counts.push((z, 1)),
        }
    }
    counts
        .iter()
        .map(|(z, n)| format!("{}{}", if *n > 1 { format!("({n}) ") } else { String::new() }, inches(z)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A sheet's view into model space: one viewport's window.
struct Window {
    key: String,
    vx: f64,
    vy: f64,
    hw: f64,
    hh: f64,
    c: f64,
    sn: f64,
}

/// `equipmentList(db, drawing)`: None when the drawing has no model space or no item tags.
pub fn equipment_list(db: &Obj, drawing: &Drawing) -> Option<EquipList> {
    let blocks = db.arr("blocks").map(Vec::as_slice).unwrap_or(&[]);
    let model = blocks.iter().filter_map(V::as_obj).find(|b| b.str("name").is_some_and(|n| n.eq_ignore_ascii_case("*model_space")))?;

    let mut windows: Vec<Window> = Vec::new();
    for s in &drawing.spaces {
        if s.kind != SpaceKind::Paper {
            continue;
        }
        for v in &s.viewports {
            let k = v.h / v.view_height;
            // !(k > 0): NaN too
            if k.is_nan() || k <= 0.0 {
                continue;
            }
            let (hw, hh) = (v.w / 2.0 / k, v.h / 2.0 / k);
            windows.push(Window { key: s.key.clone(), vx: v.vx, vy: v.vy, hw, hh, c: (-v.twist).jcos(), sn: (-v.twist).jsin() });
        }
    }
    let sheets_at = |x: f64, y: f64| -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for w in &windows {
            let (dx, dy) = (x - w.vx, y - w.vy);
            let u = dx * w.c - dy * w.sn;
            let v = dx * w.sn + dy * w.c;
            if u.abs() <= w.hw && v.abs() <= w.hh && !out.contains(&w.key) {
                out.push(w.key.clone());
            }
        }
        out
    };

    let mut rows: Vec<Row> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut tags = 0usize;
    // where the keys were in the object before (the reader writes every object of a kind in one key order)
    let (mut h_type, mut h_attribs, mut h_insert, mut h_tag, mut h_value) = (0usize, 0usize, 0usize, 0usize, 0usize);
    for e in model.arr("entities").map(Vec::as_slice).unwrap_or(&[]) {
        let Some(e) = e.as_obj() else { continue };
        let ty = e.get_at("type", &mut h_type).and_then(V::as_str).unwrap_or("");
        if ty != "INSERT" && ty != "MINSERT" {
            continue;
        }
        let attribs = e.get_at("attribs", &mut h_attribs).and_then(V::as_arr).map(Vec::as_slice).unwrap_or(&[]);
        if attribs.is_empty() {
            continue;
        }
        let mut a: Vec<(String, String)> = Vec::new();
        for t in attribs {
            let t = t.as_obj();
            let tag = t.and_then(|t| t.get_at("tag", &mut h_tag));
            if !jsv::truthy(tag) {
                continue;
            }
            // String(tag).toUpperCase() (a tag is nearly always upper-case ASCII already: no copy to make it)
            let k = match tag {
                Some(V::Str(s)) if s.bytes().all(|b| b.is_ascii() && !b.is_ascii_lowercase()) => s.clone(),
                _ => jsv::to_str(tag).to_uppercase(),
            };
            let v = match t.and_then(|t| t.get_at("value", &mut h_value)) {
                Some(V::Str(s)) => clean_str(s),
                other => clean(other),
            };
            match a.iter_mut().find(|(x, _)| *x == k) {
                Some(slot) => slot.1 = v,
                None => a.push((k, v)),
            }
        }
        let get = |k: &str| a.iter().find(|(x, _)| x == k).map(|(_, v)| v.as_str());
        match get(TAG) {
            Some(v) if !v.is_empty() => {}
            _ => continue,
        }
        if tags >= MAX_TAGS {
            break;
        }
        tags = tags.saturating_add(1);
        let or = |k: &str| get(k).unwrap_or("").to_string();
        let no = or(TAG);
        let mfr = match get("KCL-MFR-SCHDNAME") {
            Some(v) if !v.is_empty() => v.to_string(),
            _ => or("KCL-MFR"),
        };
        let key = [no.as_str(), &or("KCL-CATEGORY"), &mfr, &or("KCL-MODEL"), &or("KCL-ELEC"), &or("KCL-CW"), &or("KCL-HW")].join("\u{0}");
        let qty = or("KCL-QTY");
        let at = match index.get(&key) {
            Some(&i) => i,
            None => {
                let plumb: Vec<(&str, String)> =
                    [("cw", "KCL-CW"), ("hw", "KCL-HW"), ("dd", "KCL-DD"), ("id", "KCL-ID"), ("gas", "KCL-GAS")]
                        .iter()
                        .map(|(n, k)| (*n, plumb_size(&or(k))))
                        .collect();
                let elec = decode_elec(&or("KCL-ELEC"));
                rows.push(Row {
                    no: no.clone(),
                    qty: qty.clone(),
                    qtys: Vec::new(),
                    category: or("KCL-CATEGORY"),
                    mfr,
                    model: or("KCL-MODEL"),
                    remark: join_text(&[&or("KCL-REMARK-USER-EQUIP"), &or("KCL-REMARK")]),
                    elec_text: elec.iter().map(elec_text).collect::<Vec<_>>().join(" + "),
                    elec,
                    plumb_text: plumb
                        .iter()
                        .filter(|(_, v)| !v.is_empty())
                        .map(|(k, v)| format!("{} {v}", k.to_uppercase()))
                        .collect::<Vec<_>>()
                        .join(", "),
                    el_remark: join_text(&[&or("KCL-EL-REMARK-USER"), &or("KCL-EL-REMARK")]),
                    pl_remark: join_text(&[&or("KCL-PL-REMARK-USER"), &or("KCL-PL-REMARK")]),
                    places: Vec::new(),
                    sheets: Vec::new(),
                    clash: false,
                });
                let i = rows.len().saturating_sub(1);
                index.insert(key, i);
                i
            }
        };
        // `e.insert || {x: 0, y: 0}`
        let (x, y) = match e.get_at("insert", &mut h_insert) {
            Some(p) if p.truthy() => (jsv::to_num(jsv::get(Some(p), "x")), jsv::to_num(jsv::get(Some(p), "y"))),
            _ => (0.0, 0.0),
        };
        if let Some(r) = rows.get_mut(at) {
            r.places.push(Place { x, y, qty, sheets: sheets_at(x, y) });
        }
    }
    if tags == 0 {
        return None;
    }

    for r in &mut rows {
        let mut sheets: Vec<String> = Vec::new();
        for s in r.places.iter().flat_map(|p| p.sheets.iter()) {
            if !sheets.contains(s) {
                sheets.push(s.clone());
            }
        }
        r.sheets = sheets;
        r.qtys = r.places.iter().map(|p| p.qty.clone()).collect();
        let nums: Vec<f64> = r.qtys.iter().map(|q| js::str_to_number(q)).collect();
        r.qty = if r.qtys.len() == 1 {
            r.qtys.first().cloned().unwrap_or_default()
        } else if nums.iter().zip(&r.qtys).all(|(n, q)| !q.is_empty() && n.is_finite()) {
            js::num_to_string(nums.iter().fold(0.0, |a, b| a + b))
        } else {
            r.qtys.iter().filter(|q| !q.is_empty()).cloned().collect::<Vec<_>>().join(" + ")
        };
    }
    // the same number on different equipment where sheets show it (or everywhere, if none do)
    let mut by_no: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        match by_no.iter_mut().find(|(n, _)| *n == r.no) {
            Some((_, v)) => v.push(i),
            None => by_no.push((r.no.clone(), vec![i])),
        }
    }
    for (_, same) in &by_no {
        let on: Vec<usize> = same.iter().copied().filter(|&i| rows.get(i).is_some_and(|r| !r.sheets.is_empty())).collect();
        let group = if on.is_empty() { same.clone() } else { on };
        if group.len() > 1 {
            for i in group {
                if let Some(r) = rows.get_mut(i) {
                    r.clash = true;
                }
            }
        }
    }
    rows.sort_by(|x, y| {
        cc_io_dwg::compile::collate(&x.no, &y.no).then_with(|| cc_io_dwg::compile::collate_plain(&x.category, &y.category))
    });

    let sheets: Vec<SheetCount> = drawing
        .spaces
        .iter()
        .filter(|s| s.kind == SpaceKind::Paper)
        .map(|s| SheetCount { key: s.key.clone(), name: s.name.clone(), count: rows.iter().filter(|r| r.sheets.contains(&s.key)).count() })
        .filter(|s| s.count > 0)
        .collect();
    let off_sheet = rows.iter().filter(|r| r.sheets.is_empty()).count();
    Some(EquipList { rows, tags, sheets, off_sheet })
}

/// `equipmentCsv(rows, sheetName)`: the CSV for a spreadsheet (the caller adds the BOM); cells that start with
/// = + - @ tab or CR get a leading '.
pub fn equipment_csv(rows: &[&Row], sheet_name: &dyn Fn(&str) -> String) -> String {
    let head = [
        "Item",
        "Qty",
        "Category",
        "Manufacturer",
        "Model",
        "Electrical",
        "Plumbing",
        "Remarks",
        "Electrical remarks",
        "Plumbing remarks",
        "Sheets",
        "Placed",
    ];
    let cell = |v: &str| -> String {
        let mut s = v.to_string();
        if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
            s.insert(0, '\'');
        }
        if s.contains(['"', ',', '\r', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s }
    };
    let mut out = String::new();
    out.push_str(&head.iter().map(|h| cell(h)).collect::<Vec<_>>().join(","));
    out.push_str("\r\n");
    for r in rows {
        let line = [
            r.no.clone(),
            r.qty.clone(),
            r.category.clone(),
            r.mfr.clone(),
            r.model.clone(),
            r.elec_text.clone(),
            r.plumb_text.clone(),
            r.remark.clone(),
            r.el_remark.clone(),
            r.pl_remark.clone(),
            r.sheets.iter().map(|k| sheet_name(k)).collect::<Vec<_>>().join("; "),
            r.places.len().to_string(),
        ];
        out.push_str(&line.iter().map(|c| cell(c)).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kcl_fields() {
        cc_lang::with(cc_lang::Locale::En, || {
            let e = decode_elec("45~15~~208~3~60~X~~~");
            assert_eq!(e.len(), 1);
            assert_eq!(elec_text(&e[0]), "208 V 3 ph, 45 A, 15 kW, direct connection");
            assert!(decode_elec("~~~~").is_empty());
            let p = decode_elec("~~~120~1~60~~X~5-15P");
            assert_eq!(elec_text(&p[0]), "120 V 1 ph, cord and plug 5-15P");
            assert_eq!(plumb_size("0.5~39~~"), "1/2\"");
            assert_eq!(plumb_size("1.5~1^1.5~2"), "(2) 1 1/2\"");
            assert_eq!(plumb_size("2"), "2\"");
            assert_eq!(plumb_size("0.99"), "1\"");
            assert_eq!(plumb_size("abc"), "abc");
            assert_eq!(clean_str(" a\u{0}b\u{200b} "), "ab");
            assert_eq!(join_text(&["x", " x ", "", "y"]), "x y");
        });
    }

    #[test]
    fn csv_cells() {
        let r = Row {
            no: "=1".into(),
            qty: "2".into(),
            qtys: vec![],
            category: "a, \"b\"".into(),
            mfr: String::new(),
            model: String::new(),
            remark: String::new(),
            elec: vec![],
            elec_text: String::new(),
            plumb_text: String::new(),
            el_remark: String::new(),
            pl_remark: String::new(),
            places: vec![],
            sheets: vec!["k".into()],
            clash: false,
        };
        let s = equipment_csv(&[&r], &|k| format!("S-{k}"));
        assert!(s.ends_with("'=1,2,\"a, \"\"b\"\"\",,,,,,,,S-k,0\r\n"), "{s}");
    }
}
