//! The page's texts for the DWG view extras, as bimcraft.js and equip-view.js build them (in the language now): the
//! equipment tab and schedule, the CSV's name and the status lines, the dynamic-block list, Properties in 3D and for
//! a pick. The UI lays them out; the command line prints them (forks/tools/dwgviewdiff.mjs compares them with 0.5).

use cc_cad::js;
use cc_drawing::{Drawing, SpaceKind};
use cc_io_dwg::value::Obj;

use crate::compile3d::Model3;
use crate::dynamic::{DynInfo, DynSummary};
use crate::equipment::{EquipList, Row};
use crate::props::{attribute_rows, entity_rows};
use crate::{num, tr};

/// equip-view.js's choices: every sheet (when more than one), each sheet, not on any sheet, everything.
pub const ALL: &str = "all";
pub const OFF: &str = "off";
pub const EVERY: &str = "every";

/// Rows on sheets, or all rows when none are (the tab's number).
pub fn tab_count(list: &EquipList) -> usize {
    let on = list.rows.iter().filter(|r| !r.sheets.is_empty()).count();
    if on > 0 { on } else { list.rows.len() }
}
/// The tab above the drawing: "Equipment (n)" and its tooltip.
pub fn tab_label(list: &EquipList) -> (String, String) {
    (tr("dwg.tab.equipment", &[("n", num(tab_count(list)))]), tr("dwg.tab.equipment.title", &[]))
}
/// The Drawing panel's facts row for equipment.
pub fn info_row(list: &EquipList) -> (String, String) {
    let n = list.rows.iter().filter(|r| !r.sheets.is_empty()).count();
    (tr("info.equipment", &[]), tr("dwg.equip.onsheets", &[("n", n.to_string()), ("count", num(n))]))
}

/// One choice of "Which equipment".
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub value: String,
    pub label: String,
    pub n: usize,
    /// what the list shows: "Label (n)"
    pub text: String,
}

/// The schedule's fixed texts and choices.
#[derive(Clone, Debug, PartialEq)]
pub struct EquipHead {
    pub kind: String,
    pub title: String,
    pub intro: String,
    pub which: String,
    pub choices: Vec<Choice>,
    pub filter_placeholder: String,
    pub filter_label: String,
    pub csv: String,
    pub headers: Vec<String>,
    pub note: String,
}

/// `renderEquipment`'s head (sheet wording; drawings).
pub fn equip_head(list: &EquipList) -> EquipHead {
    equip_head_with(list, false, None)
}

/// `renderEquipment(…, { sheetWord, intro })`: `storey` words it for an IFC model's storeys; an `intro` (BimCraft's
/// placed items) takes the place of the drawing's and leaves out "Everything".
pub fn equip_head_with(list: &EquipList, storey: bool, intro: Option<String>) -> EquipHead {
    let mut choices: Vec<(String, String, usize)> = Vec::new();
    if list.sheets.len() > 1 {
        let k = if storey { "equip.every.storey" } else { "equip.every.sheet" };
        choices.push((ALL.into(), tr(k, &[]), list.rows.iter().filter(|r| !r.sheets.is_empty()).count()));
    }
    for s in &list.sheets {
        choices.push((s.key.clone(), s.name.clone(), s.count));
    }
    if list.off_sheet > 0 {
        choices.push((OFF.into(), tr(if storey { "equip.off.storey" } else { "equip.off.sheet" }, &[]), list.off_sheet));
    }
    if intro.is_none() {
        choices.push((EVERY.into(), tr("equip.everything", &[]), list.rows.len()));
    }
    let mut parts: Vec<String> = Vec::new();
    if list.off_sheet > 0 {
        parts.push(tr("equip.offsheet.note", &[("n", list.off_sheet.to_string()), ("count", num(list.off_sheet))]));
    }
    if list.rows.iter().any(|r| r.clash) {
        parts.push(tr("equip.clash.note", &[]));
    }
    parts.push(tr("equip.fields.note", &[]));
    EquipHead {
        kind: tr("ui.equipment", &[]),
        title: tr("equip.title", &[]),
        intro: intro.unwrap_or_else(|| tr("equip.intro", &[("n", list.tags.to_string()), ("count", num(list.tags))])),
        which: tr("equip.which", &[]),
        choices: choices
            .into_iter()
            .map(|(value, label, n)| Choice { text: tr("equip.option", &[("label", label.clone()), ("count", num(n))]), value, label, n })
            .collect(),
        filter_placeholder: tr("equip.filter.placeholder", &[]),
        filter_label: tr("equip.filter.label", &[]),
        csv: tr("qto.csv", &[]),
        headers: [
            "equip.col.item",
            "equip.col.qty",
            "ui.equipment",
            "equip.col.manufacturer",
            "equip.col.model",
            "equip.col.electrical",
            "equip.col.plumbing",
            "equip.col.where",
        ]
        .iter()
        .map(|k| tr(k, &[]))
        .collect(),
        note: parts.join(" "),
    }
}

/// `query.toLowerCase().split(/\s+/).filter(Boolean)`.
fn words(query: &str) -> Vec<String> {
    query.to_lowercase().split(js::is_ws).filter(|w| !w.is_empty()).map(str::to_string).collect()
}

/// The rows shown for a choice and a filter (equip-view.js `shown`).
pub fn equip_shown<'a>(list: &'a EquipList, pick: &str, query: &str) -> Vec<&'a Row> {
    let w = words(query);
    list.rows
        .iter()
        .filter(|r| {
            let in_pick = match pick {
                EVERY => true,
                ALL => !r.sheets.is_empty(),
                OFF => r.sheets.is_empty(),
                key => r.sheets.iter().any(|s| s == key),
            };
            if !in_pick {
                return false;
            }
            let hay = [&r.no, &r.category, &r.mfr, &r.model, &r.remark, &r.elec_text, &r.plumb_text]
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase();
            w.iter().all(|x| hay.contains(x.as_str()))
        })
        .collect()
}

/// The count line over the table.
pub fn count_line(n: usize, query: &str) -> String {
    tr(
        if query.is_empty() { "equip.count" } else { "equip.count.match" },
        &[("n", n.to_string()), ("count", num(n)), ("query", query.to_string())],
    )
}

/// What one table row shows (main text and the small line under it, per column).
#[derive(Clone, Debug, PartialEq)]
pub struct RowCells {
    pub item: String,
    pub qty: String,
    pub qty_sub: Option<String>,
    pub category: String,
    pub clash: Option<String>,
    pub remark: Option<String>,
    pub mfr: String,
    pub model: String,
    pub elec: String,
    pub el_remark: Option<String>,
    pub plumb: String,
    pub pl_remark: Option<String>,
    pub where_: String,
    pub where_title: String,
    pub tags: Option<String>,
}

fn some(s: &str) -> Option<String> {
    if s.is_empty() { None } else { Some(s.to_string()) }
}

/// One row's cells.
pub fn row_cells(r: &Row, sheet_name: &dyn Fn(&str) -> String) -> RowCells {
    RowCells {
        item: r.no.clone(),
        qty: r.qty.clone(),
        qty_sub: if r.qtys.len() > 1 { Some(r.qtys.join(" + ")) } else { None },
        category: if r.category.is_empty() { "—".into() } else { r.category.clone() },
        clash: if r.clash { Some(tr("equip.clash", &[])) } else { None },
        remark: some(&r.remark),
        mfr: r.mfr.clone(),
        model: r.model.clone(),
        elec: r.elec_text.clone(),
        el_remark: some(&r.el_remark),
        plumb: r.plumb_text.clone(),
        pl_remark: some(&r.pl_remark),
        where_: if r.sheets.is_empty() {
            tr("equip.modelonly", &[])
        } else {
            r.sheets.iter().map(|k| sheet_name(k)).collect::<Vec<_>>().join(", ")
        },
        where_title: tr("equip.show.title", &[("no", r.no.clone())]),
        tags: if r.places.len() > 1 {
            Some(tr("equip.tags", &[("n", r.places.len().to_string()), ("count", num(r.places.len()))]))
        } else {
            None
        },
    }
}

/// The CSV's file name: "<drawing> equipment[ (<choice>)].csv".
pub fn csv_name(file: &str, label: &str, pick: &str) -> String {
    let lower = file.to_ascii_lowercase();
    let stem =
        if lower.ends_with(".dwg") || lower.ends_with(".dxf") { file.get(..file.len().saturating_sub(4)).unwrap_or(file) } else { file };
    let part = if !label.is_empty() && pick != EVERY && pick != ALL {
        let clean: String = label.chars().filter(|c| !matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|')).collect();
        format!(" ({clean})")
    } else {
        String::new()
    };
    format!("{stem} equipment{part}.csv")
}

/// The status line after the CSV is saved.
pub fn downloaded(file: &str, n: usize) -> String {
    tr("equip.downloaded", &[("file", file.to_string()), ("n", n.to_string()), ("count", num(n))])
}

/// The status line after a jump to an item.
pub fn ringed(r: &Row, sheet: &str) -> String {
    if r.category.is_empty() {
        tr("equip.ringed", &[("no", r.no.clone()), ("sheet", sheet.to_string())])
    } else {
        tr("equip.ringed.cat", &[("no", r.no.clone()), ("category", r.category.clone()), ("sheet", sheet.to_string())])
    }
}

/// A space's name by its key (the drawing's own name for model space).
pub fn sheet_name(d: &Drawing, key: &str) -> String {
    d.spaces.iter().find(|s| s.key == key).map_or_else(|| key.to_string(), |s| s.name.clone())
}

/// dwg-view.js `focusModelPoint(x, y, sheetKey)`: the space to show a model point in (the sheet when one of its
/// viewports shows it, else model space) and the point there.
pub fn focus_target(d: &Drawing, x: f64, y: f64, sheet: Option<&str>) -> (String, f64, f64) {
    use cc_cad_geom::jsmath::JsMath;
    if let Some(s) = sheet.and_then(|k| d.spaces.iter().find(|s| s.key == k)) {
        for vp in &s.viewports {
            let k = vp.h / vp.view_height;
            let (c, sn) = (vp.twist.jcos(), vp.twist.jsin());
            let m = [k * c, k * sn, -k * sn, k * c, vp.cx - k * (c * vp.vx - sn * vp.vy), vp.cy - k * (sn * vp.vx + c * vp.vy)];
            let qx = m[0] * x + m[2] * y + m[4];
            let qy = m[1] * x + m[3] * y + m[5];
            if (qx - vp.cx).abs() <= vp.w / 2.0 && (qy - vp.cy).abs() <= vp.h / 2.0 {
                return (s.key.clone(), qx, qy);
            }
        }
    }
    let key = d.spaces.iter().find(|s| s.kind == SpaceKind::Model).map_or_else(|| "model".to_string(), |s| s.key.clone());
    (key, x, y)
}

/// The Drawing panel's dynamic-block list (None when there are none).
#[derive(Clone, Debug, PartialEq)]
pub struct DynView {
    pub title: String,
    /// (name, lines under it)
    pub items: Vec<(String, Vec<String>)>,
    pub note: String,
}

/// bimcraft.js `dynamicBlock(db)`.
pub fn dyn_view(list: &[DynSummary]) -> Option<DynView> {
    if list.is_empty() {
        return None;
    }
    let items = list
        .iter()
        .map(|d| {
            let mut lines = vec![tr("dwg.dyn.count", &[("n", d.count.to_string())])];
            if d.states.is_empty() {
                lines.push(tr("dwg.dyn.nostates", &[]));
            } else {
                let shown: Vec<String> = d
                    .shown
                    .iter()
                    .filter(|(k, _)| !k.is_empty())
                    .map(|(k, n)| tr("dwg.dyn.shown.one", &[("state", k.clone()), ("n", n.to_string())]))
                    .collect();
                let list = if shown.is_empty() { "—".to_string() } else { shown.join(", ") };
                lines.push(tr("dwg.dyn.shown", &[("list", list)]));
                let label = d.label.clone().unwrap_or_else(|| tr("dwg.dyn.label", &[]));
                lines.push(tr("dwg.dyn.states", &[("label", label), ("list", d.states.join(", "))]));
            }
            (d.name.clone(), lines)
        })
        .collect();
    Some(DynView { title: tr("dwg.dyn.title", &[]), items, note: tr("dwg.dyn.note", &[]) })
}

/// A counted list with its heading and note.
#[derive(Clone, Debug, PartialEq)]
pub struct Counts {
    pub title: String,
    pub rows: Vec<(String, String)>,
    pub note: String,
}

/// Properties while in 3D (bimcraft.js `props3d`).
#[derive(Clone, Debug, PartialEq)]
pub struct Props3d {
    pub hint: String,
    pub intro: String,
    pub rows: Vec<(String, String)>,
    pub flat: Option<Counts>,
    pub missing: Option<Counts>,
}

fn sorted(o: &[(String, u64)]) -> Vec<(String, String)> {
    let mut v: Vec<&(String, u64)> = o.iter().collect();
    v.sort_by_key(|x| std::cmp::Reverse(x.1));
    v.into_iter().map(|(k, n)| (k.clone(), num(usize::try_from(*n).unwrap_or(usize::MAX)))).collect()
}

/// `props3d(m)`.
pub fn props3d(m: &Model3) -> Props3d {
    let s = &m.stats;
    let n = |x: u64| num(usize::try_from(x).unwrap_or(usize::MAX));
    let mut rows = vec![(tr("dwg3d.lines", &[]), n(s.segments)), (tr("dwg3d.faces", &[]), n(s.triangles))];
    if m.has_bounds() {
        let f = |v: f64| if v.abs() >= 100.0 { js::to_fixed(v, 0) } else { js::to_fixed(v, 2) };
        let size: Vec<String> = m.max.iter().zip(&m.min).map(|(hi, lo)| f(hi - lo)).collect();
        rows.push((
            tr("dwg3d.size", &[]),
            format!("{}{}", size.join(" × "), if m.units.is_empty() { String::new() } else { format!(" {}", m.units) }),
        ));
    }
    if s.polyline3d_fixed > 0 {
        rows.push((tr("dwg3d.poly3d", &[]), tr("dwg3d.poly3d.fixed", &[("count", n(s.polyline3d_fixed))])));
    }
    let flat = sorted(&s.flat_only);
    let missing = sorted(&s.not_drawn);
    Props3d {
        hint: tr("dwg3d.hint", &[]),
        intro: tr("dwg.pick.intro", &[]),
        rows,
        flat: if flat.is_empty() {
            None
        } else {
            Some(Counts { title: tr("dwg3d.flat.title", &[]), rows: flat, note: tr("dwg3d.flat.note", &[]) })
        },
        missing: if missing.is_empty() {
            None
        } else {
            Some(Counts { title: tr("dwg.notdrawn.title", &[]), rows: missing, note: tr("dwg3d.notdrawn.note", &[]) })
        },
    }
}

/// The status line on switching to 3D, and whether it is an error.
pub fn shown_status(m: &Model3) -> (String, bool) {
    let s = &m.stats;
    if s.segments.saturating_add(s.triangles) == 0 {
        return (tr("dwg3d.nothing", &[]), true);
    }
    let n = |x: u64| num(usize::try_from(x).unwrap_or(usize::MAX));
    (tr(if s.truncated { "dwg3d.shown.large" } else { "dwg3d.shown" }, &[("lines", n(s.segments)), ("faces", n(s.triangles))]), false)
}

/// Properties for a pick (bimcraft.js `pick3d`).
#[derive(Clone, Debug, PartialEq)]
pub enum PickView {
    /// several: the heading, the hint, (type, count) most first
    Many { title: String, hint: String, counts: Vec<(String, String)> },
    /// one: the class line, the name, (label, value) rows, attributes (summary, rows), the way back
    One { cls: String, name: String, rows: Vec<(String, String)>, attrs: Option<(String, Vec<(String, String)>)>, back: String },
}

/// The model-space entities (`modelSpaceOf(db)`).
pub fn model_space(db: &Obj) -> &[cc_io_dwg::value::V] {
    db.arr("blocks")
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .filter_map(cc_io_dwg::value::V::as_obj)
        .find(|b| b.str("name").is_some_and(|n| n.eq_ignore_ascii_case("*model_space")))
        .and_then(|b| b.arr("entities"))
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// What Properties shows for these picked owners (indexes into the model-space entities `ents`, [`model_space`]);
/// None for an empty pick.
pub fn pick_view(
    ents: &[cc_io_dwg::value::V],
    units: &str,
    picked: &[usize],
    dyn_of: &dyn Fn(&Obj) -> Option<DynInfo>,
) -> Option<PickView> {
    if picked.len() > 1 {
        let mut by: Vec<(String, usize)> = Vec::new();
        for &o in picked {
            let ty = ents.get(o).and_then(|e| e.as_obj()).and_then(|e| e.str("type")).filter(|t| !t.is_empty()).unwrap_or("?").to_string();
            match by.iter_mut().find(|(k, _)| *k == ty) {
                Some((_, n)) => *n = n.saturating_add(1),
                None => by.push((ty, 1)),
            }
        }
        by.sort_by_key(|x| std::cmp::Reverse(x.1));
        return Some(PickView::Many {
            title: tr("dwg.pick.many", &[("n", picked.len().to_string())]),
            hint: tr("dwg.pick.hint", &[]),
            counts: by.into_iter().map(|(k, n)| (k, num(n))).collect(),
        });
    }
    let e = ents.get(*picked.first()?)?.as_obj()?;
    let dy = dyn_of(e);
    let ty = cc_io_cad::jsv::str_nn(e.get("type"));
    let rows = entity_rows(e, units, dy.as_ref())
        .into_iter()
        .map(|r| (tr(&r.key, &[]), if r.value_key { tr(&r.value, &[]) } else { r.value }))
        .collect();
    let attrs = attribute_rows(e);
    let truthy = |k: &str| cc_io_cad::jsv::truthy(e.get(k));
    let name = match &dy {
        Some(d) if !d.name.is_empty() => d.name.clone(),
        _ if truthy("block") => cc_io_cad::jsv::to_str(e.get("block")),
        _ if truthy("layer") => cc_io_cad::jsv::to_str(e.get("layer")),
        _ => ty.clone(),
    };
    Some(PickView::One {
        cls: if dy.is_some() { tr("dwg.dyn.kind", &[("type", ty.clone())]) } else { ty },
        name,
        rows,
        attrs: if attrs.is_empty() { None } else { Some((tr("dwg.prop.attributes", &[("n", attrs.len().to_string())]), attrs)) },
        back: tr("dwg.pick.back", &[]),
    })
}
