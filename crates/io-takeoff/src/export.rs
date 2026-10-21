//! `export.js` — the estimate as CSV and XLSX (live formulas with cached values: qty + waste, ROUND(…, 2) extensions,
//! SUM totals), the per-sheet quantities, marks, scales, bid areas, alternates, markups, bid summary and the TK.4
//! reports (takeoff summary, material list, quote request, by CSI division), spot elevations for CivilCraft.

use std::collections::HashSet;

use serde_json::Value;

use cc_takeoff::bid::{self, bid_summary, bid_used, normal_bid};
use cc_takeoff::collate::locale_compare;
use cc_takeoff::i18n::{F, t, tf};
use cc_takeoff::jv;
use cc_takeoff::library::{Cell, cell_of, csv_line};
use cc_takeoff::model::{
    self, Estimate, Measure, QOpts, alternates, area_estimates, base_estimate, measure_mark, quantities, rnum, rstr, sheet_breakdown,
    unit_of,
};
use cc_takeoff::units::{self, describe_scale, format_pitch, ft_in, round_to};

use crate::xlsx::{CellV, Sheet, XCell, cref};

/// A sheet key → its name in the exports.
pub type Labeler<'a> = &'a dyn Fn(&str) -> String;

/// The estimate's column headers.
pub fn estimate_head() -> Vec<String> {
    ["item", "type", "unit", "qty", "waste", "qtyw", "matu", "labu", "mat", "lab", "total", "notes"]
        .iter()
        .map(|k| t(&format!("x.head.{k}")))
        .collect()
}

fn type_label(k: &str) -> String {
    t(&format!("type.{k}"))
}

fn row_note(r: &Value) -> String {
    let mut nt: Vec<String> = Vec::new();
    let has_err = jv::truthy(r.get("error"));
    if has_err {
        nt.push(tf("x.note.error", &[("error", F::S(jv::to_string(r.get("error"))))]));
    }
    if r.get("formula").is_some() && !has_err {
        nt.push(tf("x.note.formula", &[("formula", F::S(jv::to_string(r.get("formula"))))]));
    }
    if jv::truthy(r.get("unscaled")) {
        nt.push(tf("x.note.unscaled", &[("n", F::N(rnum(r, "unscaled")))]));
    }
    for (k, key) in [("wallArea", "x.note.wall"), ("perimeter", "x.note.perimeter"), ("backout", "export.backout.note")] {
        if let Some(o) = r.get(k).filter(|o| jv::truthy(Some(o))) {
            nt.push(tf(key, &[("value", F::N(round_to(jv::to_number(o.get("value")), 2))), ("unit", F::S(jv::to_string(o.get("unit"))))]));
        }
    }
    if rstr(r, "kind") == "condition" {
        nt.push(tf("export.cond.note", &[("type", F::S(jv::lower(&type_label(rstr(r, "cond")))))]));
    }
    if let Some(w) = r.get("work").filter(|w| jv::truthy(Some(w))) {
        let crew = r.get("crew");
        let c = |k: &str| crew.map_or(f64::NAN, |c| jv::to_number(c.get(k)));
        let wn = |k: &str| jv::to_number(w.get(k));
        nt.push(tf(
            "export.crew.note",
            &[
                ("hours", F::N(round_to(wn("hours"), 2))),
                ("size", F::N(c("size"))),
                ("wh", F::N(round_to(wn("workerHours"), 2))),
                ("days", F::N(round_to(wn("days"), 2))),
                ("prod", F::N(round_to(c("prod"), 4))),
                ("unit", F::S(rstr(r, "unit").to_string())),
                ("wage", F::N(round_to(c("wage") / 10000.0, 4))),
            ],
        ));
    }
    nt.join("; ")
}

fn q4(v: f64) -> Cell {
    if v.is_finite() { Cell::Num(round_to(v, 4)) } else { Cell::Null }
}
fn dollars(cents: f64) -> f64 {
    round_to(cents / 100.0, 2)
}
fn rate(r4: f64) -> f64 {
    r4 / 10000.0
}
fn js_sign(x: f64) -> f64 {
    cc_m3d::js::sign(x)
}

/// The estimate as CSV text (values, not formulas).
pub fn estimate_csv(state: &Value, est: &Estimate) -> String {
    let head: Vec<Cell> = estimate_head().into_iter().map(Cell::Str).collect();
    let mut lines = vec![csv_line(&head)];
    for r in &est.rows {
        let comp = rstr(r, "kind") == "component";
        let name = if comp { Cell::Str(format!("  {}", jv::to_string(r.get("name")))) } else { cell_of(r.get("name")) };
        let ty = if comp { Cell::Str(t("x.component")) } else { Cell::Str(type_label(rstr(r, "type"))) };
        if jv::truthy(r.get("error")) {
            lines.push(csv_line(&[
                name,
                ty,
                cell_of(r.get("unit")),
                Cell::Null,
                Cell::Num(rnum(r, "waste") / 100.0),
                Cell::Null,
                Cell::Num(rate(rnum(r, "mat"))),
                Cell::Num(rate(rnum(r, "lab"))),
                Cell::Null,
                Cell::Null,
                Cell::Null,
                Cell::Str(row_note(r)),
            ]));
        } else {
            lines.push(csv_line(&[
                name,
                ty,
                cell_of(r.get("unit")),
                q4(rnum(r, "qty")),
                Cell::Num(rnum(r, "waste") / 100.0),
                q4(rnum(r, "qtyW")),
                Cell::Num(rate(rnum(r, "mat"))),
                Cell::Num(rate(rnum(r, "lab"))),
                Cell::Num(dollars(rnum(r, "matExt"))),
                Cell::Num(dollars(rnum(r, "labExt"))),
                Cell::Num(dollars(rnum(r, "total"))),
                Cell::Str(row_note(r)),
            ]));
        }
    }
    let alts = alternates(state);
    let nul = || Cell::Null;
    lines.push(csv_line(&[
        Cell::Str(if alts.is_empty() { t("x.total") } else { t("export.alt.base") }),
        nul(),
        nul(),
        nul(),
        nul(),
        nul(),
        nul(),
        nul(),
        Cell::Num(dollars(est.mat_total)),
        Cell::Num(dollars(est.lab_total)),
        Cell::Num(dollars(est.total)),
        Cell::Str(if est.incomplete { t("x.incomplete") } else { String::new() }),
    ]));
    if !alts.is_empty() {
        lines.push(String::new());
        lines.push(csv_line(&[
            Cell::Str(t("export.alt.h")),
            Cell::Str(t("export.alt.kind")),
            nul(),
            nul(),
            nul(),
            nul(),
            nul(),
            nul(),
            nul(),
            nul(),
            Cell::Str(t("export.alt.amount")),
            nul(),
        ]));
        for a in &alts {
            let sg = js_sign(if a.amount == 0.0 || a.amount.is_nan() { 1.0 } else { a.amount });
            lines.push(csv_line(&[
                cell_of(a.area.get("name")),
                Cell::Str(t(&format!("alt.{}", a.alt))),
                nul(),
                nul(),
                nul(),
                nul(),
                nul(),
                nul(),
                Cell::Num(dollars(a.est.mat_total * sg)),
                Cell::Num(dollars(a.est.lab_total * sg)),
                Cell::Num(dollars(a.amount)),
                Cell::Str(if a.est.incomplete { t("x.incomplete") } else { String::new() }),
            ]));
        }
    }
    if bid_used(state.get("bid")) {
        let ins: Vec<bid::AltIn> = alts.iter().map(model::Alternate::bid_in).collect();
        let b = bid_summary(est.mat_total, est.lab_total, est.incomplete, state.get("bid"), &ins);
        lines.push(String::new());
        lines.push(csv_line(&[Cell::Str(t("bid.h"))]));
        for l in &b.lines {
            let pct = l.pct.filter(|p| *p != 0.0).map_or(Cell::Null, |p| Cell::Str(format!("{}%", cc_takeoff::jv::num_str(p / 100.0))));
            lines.push(csv_line(&[
                Cell::Str(l.label.clone()),
                pct,
                nul(),
                nul(),
                nul(),
                nul(),
                nul(),
                nul(),
                nul(),
                nul(),
                Cell::Num(dollars(l.cents)),
                nul(),
            ]));
        }
        for a in &b.alts {
            lines.push(csv_line(&[
                Cell::Str(tf("bid.pdf.alt", &[("name", F::S(a.name.clone())), ("kind", F::S(jv::lower(&t(&format!("alt.{}", a.alt)))))])),
                nul(),
                nul(),
                nul(),
                nul(),
                nul(),
                nul(),
                nul(),
                nul(),
                nul(),
                Cell::Num(dollars(a.with_markup)),
                Cell::Str(t("bid.alt.note")),
            ]));
        }
    }
    lines.join("\r\n") + "\r\n"
}

/// Per-sheet quantities as CSV.
pub fn sheets_csv(state: &Value, label: Labeler<'_>) -> String {
    let head: Vec<Cell> = ["sheet", "item", "qty", "unit", "marks"].iter().map(|k| Cell::Str(t(&format!("x.head.{k}")))).collect();
    let mut lines = vec![csv_line(&head)];
    for r in sheet_breakdown(state, &quantities(state, &QOpts::default())) {
        lines.push(csv_line(&[
            Cell::Str(label(rstr(&r, "sheet"))),
            cell_of(r.get("name")),
            q4(rnum(&r, "qty")),
            cell_of(r.get("unit")),
            Cell::Num(rnum(&r, "marks")),
        ]));
    }
    lines.join("\r\n") + "\r\n"
}

/// One line of the material list.
#[derive(Debug, Clone)]
pub struct Material {
    pub name: String,
    pub unit: String,
    pub qty: f64,
    pub qty_w: f64,
    pub mat_ext: f64,
    pub csi: String,
    pub items: Vec<String>,
}

/// The material list: assembly parts and priced item / condition rows, added up by name and unit.
pub fn material_list(state: &Value, est: &Estimate) -> Vec<Material> {
    let items = jv::arr(state.get("items")).map_or(&[][..], Vec::as_slice);
    let mut by: Vec<(String, Material)> = Vec::new();
    for r in &est.rows {
        if jv::truthy(r.get("error")) || !(rnum(r, "qty") > 0.0) {
            continue;
        }
        if rstr(r, "kind") != "component" && !(rnum(r, "mat") > 0.0) {
            continue;
        }
        let it = items.iter().rev().find(|i| jv::strict_eq(i.get("id"), r.get("itemId")));
        let name = cc_m3d::js::trim(&jv::to_string(r.get("name"))).to_string();
        let unit = jv::to_string(r.get("unit"));
        let k = format!("{}\u{0}{unit}", jv::lower(&name));
        let it_csi = it.and_then(|i| i.get("csi")).filter(|c| jv::truthy(Some(c))).map(|c| jv::to_string(Some(c)));
        let i = match by.iter().position(|(kk, _)| *kk == k) {
            Some(i) => i,
            None => {
                by.push((
                    k,
                    Material {
                        name: name.clone(),
                        unit: unit.clone(),
                        qty: 0.0,
                        qty_w: 0.0,
                        mat_ext: 0.0,
                        csi: it_csi.clone().unwrap_or_default(),
                        items: Vec::new(),
                    },
                ));
                by.len().saturating_sub(1)
            }
        };
        if let Some((_, g)) = by.get_mut(i) {
            g.qty += rnum(r, "qty");
            g.qty_w += rnum(r, "qtyW");
            g.mat_ext += rnum(r, "matExt");
            if g.csi.is_empty()
                && let Some(c) = &it_csi
            {
                g.csi = c.clone();
            }
            if let Some(it) = it {
                let nm = jv::to_string(it.get("name"));
                if !g.items.contains(&nm) {
                    g.items.push(nm);
                }
            }
        }
    }
    let mut out: Vec<Material> = by.into_iter().map(|(_, g)| g).collect();
    cc_m3d::js::sort_by(&mut out, |a, b| {
        let ca = if a.csi.is_empty() { "99" } else { a.csi.as_str() };
        let cb = if b.csi.is_empty() { "99" } else { b.csi.as_str() };
        let c = locale_compare(ca, cb);
        if c != 0.0 { c } else { locale_compare(&a.name, &b.name) }
    });
    out
}

/// One row of the takeoff summary.
#[derive(Debug, Clone)]
pub struct SummaryRow {
    pub name: String,
    pub csi: String,
    pub ty: String,
    pub qty: f64,
    pub unit: String,
    pub marks: f64,
    pub sheets: f64,
}
/// Quantities only, no prices: one row per item and second condition.
pub fn takeoff_summary(state: &Value, est: &Estimate) -> Vec<SummaryRow> {
    let mut sheets_of: Vec<(String, HashSet<String>)> = Vec::new();
    for m in jv::arr(state.get("marks")).map_or(&[][..], Vec::as_slice) {
        let k = format!("{}\u{0}{}", jv::to_string(m.get("item")), jv::to_string(m.get("kind")));
        let sh = jv::to_string(m.get("sheet"));
        match sheets_of.iter_mut().find(|(kk, _)| *kk == k) {
            Some((_, s)) => {
                s.insert(sh);
            }
            None => sheets_of.push((k, HashSet::from([sh]))),
        }
    }
    let items = jv::arr(state.get("items")).map_or(&[][..], Vec::as_slice);
    est.rows
        .iter()
        .filter(|r| matches!(rstr(r, "kind"), "item" | "condition"))
        .map(|r| {
            let it = items.iter().rev().find(|i| jv::strict_eq(i.get("id"), r.get("itemId")));
            let cond = rstr(r, "kind") == "condition";
            let kind = if cond { rstr(r, "cond").to_string() } else { it.map(|i| jv::to_string(i.get("type"))).unwrap_or_default() };
            let name = if cond {
                tf(
                    "qty.cond.name",
                    &[("item", F::S(jv::to_string(r.get("name")))), ("type", F::S(jv::lower(&type_label(rstr(r, "cond")))))],
                )
            } else {
                jv::to_string(r.get("name"))
            };
            let k = format!("{}\u{0}{kind}", rstr(r, "itemId"));
            SummaryRow {
                name,
                csi: it.and_then(|i| i.get("csi")).filter(|c| jv::truthy(Some(c))).map(|c| jv::to_string(Some(c))).unwrap_or_default(),
                ty: type_label(&kind),
                qty: rnum(r, "qty"),
                unit: jv::to_string(r.get("unit")),
                marks: if jv::truthy(r.get("marks")) { rnum(r, "marks") } else { 0.0 },
                sheets: sheets_of.iter().find(|(kk, _)| *kk == k).map_or(0.0, |(_, s)| s.len() as f64),
            }
        })
        .collect()
}

/// The CSI MasterFormat divisions the estimate groups by.
pub const CSI_DIVISIONS: [&str; 35] = [
    "00", "01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12", "13", "14", "21", "22", "23", "25", "26", "27", "28",
    "31", "32", "33", "34", "35", "40", "41", "42", "43", "44", "45", "46", "48",
];
pub fn division_title(d: &str) -> String {
    if CSI_DIVISIONS.contains(&d) { t(&format!("csi.div.{d}")) } else { t("csi.div.other") }
}
/// One division of the estimate.
#[derive(Debug, Clone)]
pub struct Division {
    pub div: String,
    pub title: String,
    pub mat: f64,
    pub lab: f64,
    pub total: f64,
    pub items: f64,
}
pub fn by_division(state: &Value, est: &Estimate) -> Vec<Division> {
    let items = jv::arr(state.get("items")).map_or(&[][..], Vec::as_slice);
    let mut by: Vec<(String, Division, HashSet<String>)> = Vec::new();
    for r in &est.rows {
        if jv::truthy(r.get("error")) {
            continue;
        }
        let it = items.iter().rev().find(|i| jv::strict_eq(i.get("id"), r.get("itemId")));
        let div = it
            .and_then(|i| i.get("csi"))
            .filter(|c| jv::truthy(Some(c)))
            .map(|c| cc_m3d::js::slice16(&jv::to_string(Some(c)), 2))
            .unwrap_or_default();
        let i = match by.iter().position(|(k, _, _)| *k == div) {
            Some(i) => i,
            None => {
                let title = if div.is_empty() { t("csi.div.none") } else { division_title(&div) };
                by.push((div.clone(), Division { div: div.clone(), title, mat: 0.0, lab: 0.0, total: 0.0, items: 0.0 }, HashSet::new()));
                by.len().saturating_sub(1)
            }
        };
        if let Some((_, g, set)) = by.get_mut(i) {
            g.mat += rnum(r, "matExt");
            g.lab += rnum(r, "labExt");
            g.total += rnum(r, "total");
            set.insert(rstr(r, "itemId").to_string());
        }
    }
    let mut out: Vec<Division> = by
        .into_iter()
        .map(|(_, mut g, set)| {
            g.items = set.len() as f64;
            g
        })
        .collect();
    cc_m3d::js::sort_by(&mut out, |a, b| {
        locale_compare(if a.div.is_empty() { "99" } else { &a.div }, if b.div.is_empty() { "99" } else { &b.div })
    });
    out
}

/// Spot elevations for CivilCraft: `Ok((csv, n))` or why not.
pub fn spot_elevations_csv(state: &Value, sheet: &str, label: Labeler<'_>) -> Result<(String, usize), String> {
    let sc = state.get("sheets").and_then(|s| s.get(sheet)).and_then(|s| s.get("scale")).filter(|s| jv::truthy(Some(s)));
    let marks: Vec<&Value> = jv::arr(state.get("marks"))
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .filter(|m| jv::str(m.get("sheet")) == Some(sheet) && jv::str(m.get("kind")) == Some("count") && jv::num(m.get("z")).is_some())
        .collect();
    if marks.is_empty() {
        return Err(t("spots.none"));
    }
    let Some(sc) = sc else { return Err(tf("spots.noscale", &[("sheet", F::S(label(sheet)))])) };
    let k = jv::to_number(sc.get("ftPerUnit"));
    let items = jv::arr(state.get("items")).map_or(&[][..], Vec::as_slice);
    let mut lines = vec![csv_line(&["Point", "Northing", "Easting", "Elevation", "Description"].map(Cell::from))];
    for (i, m) in marks.iter().enumerate() {
        let p0 = jv::arr(m.get("pts")).and_then(|l| l.first());
        let px = p0.map_or(f64::NAN, |p| jv::to_number(p.get("x")));
        let py = p0.map_or(f64::NAN, |p| jv::to_number(p.get("y")));
        let name = items
            .iter()
            .rev()
            .find(|it| jv::strict_eq(it.get("id"), m.get("item")))
            .and_then(|it| it.get("name"))
            .filter(|n| jv::truthy(Some(n)))
            .map_or(String::new(), |n| jv::to_string(Some(n)));
        lines.push(csv_line(&[
            Cell::Num(i as f64 + 1.0),
            Cell::Num(round_to(py * k, 4)),
            Cell::Num(round_to(px * k, 4)),
            Cell::Num(round_to(jv::to_number(m.get("z")), 4)),
            Cell::Str(name),
        ]));
    }
    Ok((lines.join("\r\n") + "\r\n", marks.len()))
}

/// The material list as CSV.
pub fn material_csv(state: &Value, est: &Estimate) -> String {
    let head: Vec<Cell> =
        ["part", "csi", "unit", "qty", "qtyw", "mat", "items"].iter().map(|k| Cell::Str(t(&format!("mat.head.{k}")))).collect();
    let mut lines = vec![csv_line(&head)];
    let list = material_list(state, est);
    for g in &list {
        lines.push(csv_line(&[
            Cell::Str(g.name.clone()),
            Cell::Str(g.csi.clone()),
            Cell::Str(g.unit.clone()),
            q4(g.qty),
            q4(g.qty_w),
            Cell::Num(dollars(g.mat_ext)),
            Cell::Str(g.items.join("; ")),
        ]));
    }
    let tot = list.iter().fold(0.0, |s, g| s + g.mat_ext);
    lines.push(csv_line(&[
        Cell::Str(t("x.total")),
        Cell::Null,
        Cell::Null,
        Cell::Null,
        Cell::Null,
        Cell::Num(dollars(tot)),
        Cell::Str(if est.incomplete { t("x.incomplete") } else { String::new() }),
    ]));
    lines.join("\r\n") + "\r\n"
}

fn markup_rows(est: &Estimate, bid_raw: Option<&Value>, total_row: usize, first_row: usize) -> Vec<Vec<XCell>> {
    let b = normal_bid(bid_raw);
    let sum = bid_summary(est.mat_total, est.lab_total, est.incomplete, Some(&b), &[]);
    let mut rows: Vec<Vec<XCell>> = Vec::new();
    let mut at: Vec<(&'static str, usize)> = Vec::new();
    let mut r = first_row;
    let push =
        |rows: &mut Vec<Vec<XCell>>, at: &mut Vec<(&'static str, usize)>, r: &mut usize, cells: Vec<XCell>, key: Option<&'static str>| {
            rows.push(cells);
            if let Some(k) = key {
                at.push((k, *r));
            }
            *r = r.saturating_add(1);
        };
    push(&mut rows, &mut at, &mut r, vec![XCell::ts(t("bid.h"), "bold")], None);
    let ii = format!("I{total_row}");
    let jj = format!("J{total_row}");
    let line = |k: &str| sum.line(k).cloned();
    let pct_row = |r: usize, key: &'static str, base_f: String, base_v: f64| -> Vec<XCell> {
        let l = line(key).unwrap_or(bid::Line { key, label: String::new(), pct: Some(0.0), cents: 0.0, sub: false });
        vec![
            XCell::t(l.label.clone()),
            XCell::Empty,
            XCell::Empty,
            XCell::Empty,
            XCell::vs(l.pct.unwrap_or(0.0) / 10000.0, "pct"),
            XCell::Empty,
            XCell::Empty,
            XCell::Empty,
            XCell::fs(dollars(base_v), base_f, "money"),
            XCell::Empty,
            XCell::fs(dollars(l.cents), format!("ROUND(I{r}*E{r},2)"), "money"),
            XCell::t(t("bid.x.base")),
        ]
    };
    if line("burden").is_some() {
        let cells = pct_row(r, "burden", jj.clone(), est.lab_total);
        push(&mut rows, &mut at, &mut r, cells, Some("burden"));
    }
    let burden_at = at.iter().find(|(k, _)| *k == "burden").map(|(_, v)| *v);
    let mut cost_f = vec![ii.clone(), jj.clone()];
    if let Some(b) = burden_at {
        cost_f.push(format!("K{b}"));
    }
    let cost = line("cost");
    push(
        &mut rows,
        &mut at,
        &mut r,
        vec![
            XCell::ts(cost.as_ref().map(|l| l.label.clone()).unwrap_or_default(), "bold"),
            XCell::Empty,
            XCell::Empty,
            XCell::Empty,
            XCell::Empty,
            XCell::Empty,
            XCell::Empty,
            XCell::Empty,
            XCell::Empty,
            XCell::Empty,
            XCell::fs(dollars(cost.map_or(0.0, |l| l.cents)), cost_f.join("+"), "boldMoney"),
            XCell::Empty,
        ],
        Some("cost"),
    );
    let mut above: Vec<&'static str> = vec!["cost"];
    let refs = |above: &[&'static str], at: &[(&'static str, usize)]| -> String {
        above.iter().filter_map(|k| at.iter().find(|(kk, _)| kk == k).map(|(_, v)| format!("K{v}"))).collect::<Vec<_>>().join("+")
    };
    let cents = |above: &[&'static str]| -> f64 {
        above.iter().fold(0.0, |n, k| n + line(k).map_or(0.0, |l| if l.cents == 0.0 || l.cents.is_nan() { 0.0 } else { l.cents }))
    };
    if line("tax").is_some() {
        let cells = pct_row(r, "tax", ii.clone(), est.mat_total);
        push(&mut rows, &mut at, &mut r, cells, Some("tax"));
        above.push("tax");
    }
    for k in ["overhead", "profit", "bond"] {
        if line(k).is_some() {
            let cells = pct_row(r, k, refs(&above, &at), cents(&above));
            push(&mut rows, &mut at, &mut r, cells, Some(k));
            above.push(k);
        }
    }
    let total = line("total");
    let f = refs(&above, &at);
    push(
        &mut rows,
        &mut at,
        &mut r,
        vec![
            XCell::ts(total.as_ref().map(|l| l.label.clone()).unwrap_or_default(), "bold"),
            XCell::Empty,
            XCell::Empty,
            XCell::Empty,
            XCell::Empty,
            XCell::Empty,
            XCell::Empty,
            XCell::Empty,
            XCell::Empty,
            XCell::Empty,
            XCell::fs(dollars(total.map_or(0.0, |l| l.cents)), f, "boldMoney"),
            XCell::Empty,
        ],
        Some("total"),
    );
    rows
}

fn head_row(keys: &[&str], prefix: &str) -> Vec<XCell> {
    keys.iter().map(|k| XCell::ts(t(&format!("{prefix}{k}")), "head")).collect()
}
fn bold_name(r: &Value) -> XCell {
    if rstr(r, "kind") == "component" {
        XCell::tv(format!("    {}", jv::to_string(r.get("name"))))
    } else {
        XCell::ts(jv::to_string(r.get("name")), "bold")
    }
}
fn xcell_of(v: Option<&Value>) -> XCell {
    match v {
        None | Some(Value::Null) => XCell::Empty,
        Some(Value::Number(x)) => XCell::Num(x.as_f64().unwrap_or(f64::NAN)),
        Some(Value::String(s)) => XCell::t(s.clone()),
        Some(other) => XCell::t(jv::to_string(Some(other))),
    }
}

/// Workbook sheets: Estimate (with formulas), By sheet, Marks, Scales, and the others when they apply.
pub fn estimate_sheets(state: &Value, label: Labeler<'_>, est: &Estimate) -> Vec<Sheet> {
    let mut rows: Vec<Vec<XCell>> = vec![estimate_head().into_iter().map(|h| XCell::ts(h, "head")).collect()];
    for r in &est.rows {
        let n = rows.len();
        let name = bold_name(r);
        let ty = if rstr(r, "kind") == "component" { XCell::t(t("x.component")) } else { XCell::t(type_label(rstr(r, "type"))) };
        if jv::truthy(r.get("error")) {
            rows.push(vec![
                name,
                ty,
                xcell_of(r.get("unit")),
                XCell::Empty,
                XCell::vs(rnum(r, "waste") / 10000.0, "pct"),
                XCell::Empty,
                XCell::vs(rate(rnum(r, "mat")), "rate"),
                XCell::vs(rate(rnum(r, "lab")), "rate"),
                XCell::Empty,
                XCell::Empty,
                XCell::Empty,
                XCell::t(row_note(r)),
            ]);
            continue;
        }
        let (d, e, f, g, h, i, j) = (cref(n, 3), cref(n, 4), cref(n, 5), cref(n, 6), cref(n, 7), cref(n, 8), cref(n, 9));
        let lab_f = if jv::truthy(r.get("work")) {
            let crew = r.get("crew");
            let c = |k: &str| crew.map_or(f64::NAN, |c| jv::to_number(c.get(k)));
            format!(
                "ROUND({f}/{}*{}*{},2)",
                cc_takeoff::jv::num_str(c("prod")),
                cc_takeoff::jv::num_str(c("size")),
                cc_takeoff::jv::num_str(rate(c("wage")))
            )
        } else {
            format!("ROUND({f}*{h},2)")
        };
        rows.push(vec![
            name,
            ty,
            xcell_of(r.get("unit")),
            XCell::vs(rnum(r, "qty"), "qty"),
            XCell::vs(rnum(r, "waste") / 10000.0, "pct"),
            XCell::fs(rnum(r, "qtyW"), format!("{d}*(1+{e})"), "qty"),
            XCell::vs(rate(rnum(r, "mat")), "rate"),
            XCell::vs(rate(rnum(r, "lab")), "rate"),
            XCell::fs(dollars(rnum(r, "matExt")), format!("ROUND({f}*{g},2)"), "money"),
            XCell::fs(dollars(rnum(r, "labExt")), lab_f, "money"),
            XCell::fs(dollars(rnum(r, "total")), format!("{i}+{j}"), "money"),
            XCell::t(row_note(r)),
        ]);
    }
    let last = rows.len();
    rows.push(Vec::new());
    let has_alt = jv::arr(state.get("areas")).is_some_and(|l| l.iter().any(|a| jv::truthy(a.get("alt"))));
    rows.push(vec![
        XCell::ts(if has_alt { t("export.alt.base") } else { t("x.total") }, "bold"),
        XCell::Empty,
        XCell::Empty,
        XCell::Empty,
        XCell::Empty,
        XCell::Empty,
        XCell::Empty,
        XCell::Empty,
        XCell::fs(dollars(est.mat_total), format!("SUM(I2:I{last})"), "boldMoney"),
        XCell::fs(dollars(est.lab_total), format!("SUM(J2:J{last})"), "boldMoney"),
        XCell::fs(dollars(est.total), format!("SUM(K2:K{last})"), "boldMoney"),
        XCell::t(if est.incomplete { t("x.incomplete") } else { String::new() }),
    ]);
    if bid_used(state.get("bid")) {
        rows.push(Vec::new());
        let total_row = rows.len().saturating_sub(1);
        let first_row = rows.len().saturating_add(1);
        rows.extend(markup_rows(est, state.get("bid"), total_row, first_row));
    }

    let q = quantities(state, &QOpts::default());
    let mut by_sheet = vec![head_row(&["sheet", "item", "qty", "unit", "marks"], "x.head.")];
    for r in sheet_breakdown(state, &q) {
        by_sheet.push(vec![
            XCell::t(label(rstr(&r, "sheet"))),
            xcell_of(r.get("name")),
            XCell::vs(rnum(&r, "qty"), "qty"),
            xcell_of(r.get("unit")),
            XCell::vs(rnum(&r, "marks"), "int"),
        ]);
    }

    let areas = jv::arr(state.get("areas")).map_or(&[][..], Vec::as_slice);
    let with_areas = !areas.is_empty();
    let area_name =
        |id: &str| areas.iter().rev().find(|a| jv::str(a.get("id")) == Some(id)).map(|a| xcell_of(a.get("name"))).unwrap_or(XCell::Empty);
    let mut marks_rows = vec![head_row(&["sheet", "item", "type", "qty", "unit", "detail"], "x.head.")];
    if with_areas && let Some(h) = marks_rows.first_mut() {
        h.push(XCell::ts(t("export.areas.area"), "head"));
    }
    let groups = jv::arr(state.get("groups")).map_or(&[][..], Vec::as_slice);
    let add_area = |m: &Value, row: Vec<XCell>| -> Vec<XCell> {
        let mut row = row;
        if let Some(gid) = m.get("group").filter(|g| jv::truthy(Some(g)))
            && let Some(g) = groups.iter().rev().find(|g| jv::strict_eq(g.get("id"), Some(gid)))
        {
            let note =
                tf("export.group.note", &[("name", F::S(jv::to_string(g.get("name")))), ("n", F::N(jv::to_number(g.get("repeat"))))]);
            let r5 = row.get(5).cloned().unwrap_or(XCell::Empty);
            let parts: Vec<String> = [r5, XCell::t(note)]
                .iter()
                .filter_map(|c| match c {
                    XCell::Empty => None,
                    XCell::Str(s) if s.is_empty() => None,
                    XCell::Str(s) => Some(s.clone()),
                    XCell::Num(x) => (*x != 0.0 && !x.is_nan()).then(|| cc_takeoff::jv::num_str(*x)),
                    XCell::Obj { .. } => Some("[object Object]".into()),
                })
                .collect();
            let mut nr: Vec<XCell> = row.iter().take(5).cloned().collect();
            nr.push(XCell::t(parts.join("; ")));
            nr.extend(row.iter().skip(6).cloned());
            row = nr;
        }
        if with_areas {
            while row.len() < 6 {
                row.push(XCell::Empty);
            }
            let a = m.get("area").filter(|a| jv::truthy(Some(a))).map(|a| jv::to_string(Some(a)));
            row.push(match a {
                Some(id) => area_name(&id),
                None => XCell::t(t("export.areas.none")),
            });
        }
        row
    };
    let items = jv::arr(state.get("items")).map_or(&[][..], Vec::as_slice);
    for m in jv::arr(state.get("marks")).map_or(&[][..], Vec::as_slice) {
        let Some(it) = items.iter().rev().find(|i| jv::strict_eq(i.get("id"), m.get("item"))) else { continue };
        let sheet = jv::to_string(m.get("sheet"));
        let fpu = state
            .get("sheets")
            .and_then(|s| s.get(&sheet))
            .and_then(|s| s.get("scale"))
            .filter(|s| jv::truthy(Some(s)))
            .map_or(f64::NAN, |s| jv::to_number(s.get("ftPerUnit")));
        let r = measure_mark(m, fpu, Some(it));
        let kind = jv::to_string(m.get("kind"));
        let mut row = vec![XCell::t(label(&sheet)), xcell_of(it.get("name")), XCell::t(type_label(&kind))];
        let unit = unit_of(it, &kind);
        let Some(r) = r else {
            row.extend([XCell::Empty, XCell::Empty, XCell::t(t("x.noscale.sheet"))]);
            marks_rows.push(add_area(m, row));
            continue;
        };
        match r {
            Measure::Count => {
                let det = match jv::num(m.get("z")) {
                    Some(z) => tf("spots.detail", &[("z", F::N(round_to(z, 4)))]),
                    None => String::new(),
                };
                row.extend([XCell::vs(1.0, "int"), XCell::t("EA"), XCell::t(det)]);
            }
            Measure::Linear { length_ft, gross_ft, deduct_ft, segments_ft, sloped_ft, slopes, deducts, .. } => {
                let k = if units::unit(&unit).is_some_and(|u| u.system == "metric") { 0.3048 } else { 1.0 };
                let seg_len = |i: usize| -> String {
                    let (sk, sp) = slopes.get(i).cloned().unwrap_or(("flat".into(), 0.0));
                    if sk == "flat" {
                        ft_in(segments_ft.get(i).copied().unwrap_or(f64::NAN))
                    } else {
                        tf(
                            "export.slope.seg",
                            &[
                                ("len", F::S(ft_in(sloped_ft.get(i).copied().unwrap_or(f64::NAN)))),
                                ("kind", F::S(jv::lower(&t(if sk == "hip" { "slope.hip" } else { "slope.rake" })))),
                                ("pitch", F::S(format_pitch(sp))),
                            ],
                        )
                    }
                };
                let segs: Vec<String> = (0..segments_ft.len())
                    .map(|i| if deducts.contains(&(i as f64)) { tf("export.deduct.seg", &[("len", F::S(seg_len(i)))]) } else { seg_len(i) })
                    .collect();
                let mut det = tf("x.segments", &[("n", F::from(segments_ft.len())), ("list", F::S(segs.join(", ")))]);
                if !deducts.is_empty() {
                    det.push_str("; ");
                    det.push_str(&tf("export.deduct.total", &[("cut", F::S(ft_in(deduct_ft))), ("gross", F::S(ft_in(gross_ft)))]));
                }
                row.extend([XCell::vs(length_ft * k, "qty"), XCell::t(if k == 1.0 { "LF" } else { "m" }), XCell::t(det)]);
            }
            Measure::Area { area_sqft, plan_sqft, factor, pitch, gross_sqft, cut_sqft, perimeter_ft, .. } => {
                let u = units::unit(&unit);
                let k = u.map_or(f64::NAN, |u| u.per_sqft);
                let slope_note = if factor > 1.0 {
                    tf("export.slope.area", &[("pitch", F::S(format_pitch(pitch))), ("plan", F::N(round_to(plan_sqft * k, 2)))]) + "; "
                } else {
                    String::new()
                };
                let holes_n = jv::arr(m.get("holes")).map_or(0, Vec::len);
                let gross = if holes_n > 0 {
                    tf("x.gross", &[("gross", F::N(round_to(gross_sqft * k, 2))), ("cut", F::N(round_to(cut_sqft * k, 2)))]) + "; "
                } else {
                    String::new()
                };
                let det = format!("{slope_note}{gross}{}", tf("x.perimeter", &[("len", F::S(ft_in(perimeter_ft)))]));
                row.extend([XCell::vs(area_sqft * k, "qty"), XCell::t(u.map_or(String::new(), |u| u.label.to_string())), XCell::t(det)]);
            }
        }
        marks_rows.push(add_area(m, row));
    }

    let mut scales = vec![head_row(&["sheet", "scale", "how", "ftpu"], "x.head.")];
    let mark_sheets: HashSet<String> =
        jv::arr(state.get("marks")).map_or(&[][..], Vec::as_slice).iter().map(|m| jv::to_string(m.get("sheet"))).collect();
    for f in jv::arr(state.get("files")).map_or(&[][..], Vec::as_slice) {
        let pages = jv::to_number(f.get("pages"));
        let pages = if pages.is_nan() { 0.0 } else { pages };
        let hash = jv::to_string(f.get("hash"));
        let mut p = 0.0;
        while p < pages {
            let key = format!("{hash}:{}", cc_takeoff::jv::num_str(p));
            p += 1.0;
            let sc = state.get("sheets").and_then(|s| s.get(&key)).and_then(|s| s.get("scale")).filter(|s| jv::truthy(Some(s)));
            if sc.is_none() && !mark_sheets.contains(&key) {
                continue;
            }
            let row = match sc {
                Some(sc) => {
                    let lbl = sc
                        .get("label")
                        .filter(|l| jv::truthy(Some(l)))
                        .map_or_else(|| describe_scale(jv::to_number(sc.get("ftPerUnit")), 1.0), |l| jv::to_string(Some(l)));
                    let how = if jv::str(sc.get("method")) == Some("preset") {
                        t("x.preset")
                    } else if let Some(cal) = sc.get("cal").filter(|c| jv::truthy(Some(c))) {
                        tf("x.calibrated.on", &[("text", F::S(jv::to_string(cal.get("text"))))])
                    } else {
                        t("x.calibrated")
                    };
                    vec![XCell::t(label(&key)), XCell::t(lbl), XCell::t(how), xcell_of(sc.get("ftPerUnit"))]
                }
                None => vec![XCell::t(label(&key)), XCell::t(t("x.noscale")), XCell::Empty, XCell::Empty],
            };
            scales.push(row);
        }
    }
    let mut out = vec![
        Sheet {
            name: t("x.sheet.estimate"),
            rows,
            cols: vec![34.0, 10.0, 7.0, 12.0, 9.0, 14.0, 16.0, 14.0, 14.0, 14.0, 14.0, 48.0],
            freeze: 1,
        },
        Sheet { name: t("x.sheet.bysheet"), rows: by_sheet, cols: vec![40.0, 34.0, 12.0, 7.0, 8.0], freeze: 1 },
        Sheet { name: t("x.sheet.marks"), rows: marks_rows, cols: vec![40.0, 34.0, 9.0, 12.0, 7.0, 60.0], freeze: 1 },
        Sheet { name: t("x.sheet.scales"), rows: scales, cols: vec![40.0, 18.0, 30.0, 18.0], freeze: 1 },
    ];
    if with_areas {
        out.push(Sheet {
            name: t("export.areas.sheet"),
            rows: area_rows(state),
            cols: vec![30.0, 34.0, 7.0, 12.0, 14.0, 14.0, 14.0, 14.0, 40.0],
            freeze: 1,
        });
    }
    let alts = alternates(state);
    if !alts.is_empty() {
        out.push(Sheet {
            name: t("export.alt.sheet"),
            rows: alternate_rows(est, &alts),
            cols: vec![34.0, 34.0, 7.0, 12.0, 14.0, 40.0],
            freeze: 1,
        });
    }
    if let Some(notes) = jv::arr(state.get("notes")).filter(|l| !l.is_empty()) {
        let mut rows_n = vec![
            ["x.head.sheet", "note.f.kind", "note.f.status", "note.f.text"].iter().map(|k| XCell::ts(t(k), "head")).collect::<Vec<_>>(),
        ];
        for nn in notes {
            rows_n.push(vec![
                XCell::t(label(&jv::to_string(nn.get("sheet")))),
                XCell::t(t(&format!("tool.{}", jv::to_string(nn.get("kind"))))),
                XCell::t(t(&format!("note.status.{}", jv::to_string(nn.get("status"))))),
                xcell_of(nn.get("text")),
            ]);
        }
        out.push(Sheet { name: t("notes.sheet"), rows: rows_n, cols: vec![40.0, 12.0, 12.0, 80.0], freeze: 1 });
    }
    if bid_used(state.get("bid")) {
        let ins: Vec<bid::AltIn> = alts.iter().map(model::Alternate::bid_in).collect();
        let b = bid_summary(est.mat_total, est.lab_total, est.incomplete, state.get("bid"), &ins);
        let mut rows_b =
            vec![vec![XCell::ts(t("bid.h"), "head"), XCell::ts(t("bid.col.pct"), "head"), XCell::ts(t("bid.col.amount"), "head")]];
        for l in &b.lines {
            rows_b.push(vec![
                if l.sub { XCell::ts(l.label.clone(), "bold") } else { XCell::t(l.label.clone()) },
                l.pct.filter(|p| *p != 0.0).map_or(XCell::Empty, |p| XCell::vs(p / 10000.0, "pct")),
                XCell::vs(dollars(l.cents), if l.sub { "boldMoney" } else { "money" }),
            ]);
        }
        if !b.alts.is_empty() {
            rows_b.push(Vec::new());
            for a in &b.alts {
                rows_b.push(vec![
                    XCell::t(tf(
                        "bid.pdf.alt",
                        &[("name", F::S(a.name.clone())), ("kind", F::S(jv::lower(&t(&format!("alt.{}", a.alt)))))],
                    )),
                    XCell::Empty,
                    XCell::vs(dollars(a.with_markup), "money"),
                ]);
            }
        }
        out.push(Sheet { name: t("bid.sheet"), rows: rows_b, cols: vec![40.0, 10.0, 16.0], freeze: 1 });
    }
    let ts = takeoff_summary(state, est);
    if !ts.is_empty() {
        let mut rows_t = vec![head_row(&["item", "csi", "type", "qty", "unit", "marks", "sheets"], "sum.head.")];
        for x in &ts {
            rows_t.push(vec![
                XCell::ts(x.name.clone(), "bold"),
                XCell::t(x.csi.clone()),
                XCell::t(x.ty.clone()),
                XCell::vs(x.qty, "qty"),
                XCell::t(x.unit.clone()),
                XCell::vs(x.marks, "int"),
                XCell::vs(x.sheets, "int"),
            ]);
        }
        out.push(Sheet { name: t("sum.sheet"), rows: rows_t, cols: vec![40.0, 11.0, 9.0, 12.0, 7.0, 8.0, 8.0], freeze: 1 });
    }
    let ml = material_list(state, est);
    if !ml.is_empty() {
        let mut rows_m = vec![head_row(&["part", "csi", "unit", "qty", "qtyw", "mat", "items"], "mat.head.")];
        for g in &ml {
            rows_m.push(vec![
                XCell::ts(g.name.clone(), "bold"),
                XCell::t(g.csi.clone()),
                XCell::t(g.unit.clone()),
                XCell::vs(g.qty, "qty"),
                XCell::vs(g.qty_w, "qty"),
                XCell::vs(dollars(g.mat_ext), "money"),
                XCell::t(g.items.join("; ")),
            ]);
        }
        rows_m.push(Vec::new());
        let tot = ml.iter().fold(0.0, |s, g| s + g.mat_ext);
        rows_m.push(vec![
            XCell::ts(t("x.total"), "bold"),
            XCell::Empty,
            XCell::Empty,
            XCell::Empty,
            XCell::Empty,
            XCell::fs(dollars(tot), format!("SUM(F2:F{})", ml.len().saturating_add(1)), "boldMoney"),
            XCell::t(if est.incomplete { t("x.incomplete") } else { String::new() }),
        ]);
        out.push(Sheet { name: t("mat.sheet"), rows: rows_m, cols: vec![40.0, 11.0, 8.0, 12.0, 14.0, 14.0, 50.0], freeze: 1 });
        let name = state.get("name").filter(|n| jv::truthy(Some(n))).map_or_else(|| "Takeoff".to_string(), |n| jv::to_string(Some(n)));
        let mut rows_q = vec![
            vec![XCell::ts(tf("quote.title", &[("name", F::S(name))]), "bold")],
            vec![XCell::t(t("quote.note"))],
            head_row(&["part", "unit", "qty", "price", "ext"], "quote.head."),
        ];
        for g in &ml {
            let nr = rows_q.len().saturating_add(1);
            rows_q.push(vec![
                XCell::t(g.name.clone()),
                XCell::t(g.unit.clone()),
                XCell::vs(g.qty_w, "qty"),
                XCell::vs(0.0, "rate"),
                XCell::fs(0.0, format!("ROUND(C{nr}*D{nr},2)"), "money"),
            ]);
        }
        rows_q.push(Vec::new());
        rows_q.push(vec![
            XCell::ts(t("x.total"), "bold"),
            XCell::Empty,
            XCell::Empty,
            XCell::Empty,
            XCell::fs(0.0, format!("SUM(E4:E{})", ml.len().saturating_add(3)), "boldMoney"),
        ]);
        out.push(Sheet { name: t("quote.sheet"), rows: rows_q, cols: vec![44.0, 8.0, 14.0, 14.0, 16.0], freeze: 3 });
    }
    if items.iter().any(|i| jv::truthy(i.get("csi"))) {
        let bd = by_division(state, est);
        let mut rows_d = vec![head_row(&["div", "title", "items", "mat", "lab", "total"], "div.head.")];
        for g in &bd {
            rows_d.push(vec![
                XCell::t(g.div.clone()),
                XCell::ts(g.title.clone(), "bold"),
                XCell::vs(g.items, "int"),
                XCell::vs(dollars(g.mat), "money"),
                XCell::vs(dollars(g.lab), "money"),
                XCell::vs(dollars(g.total), "money"),
            ]);
        }
        rows_d.push(Vec::new());
        let nn = bd.len().saturating_add(1);
        rows_d.push(vec![
            XCell::Empty,
            XCell::ts(t("x.total"), "bold"),
            XCell::Empty,
            XCell::fs(dollars(est.mat_total), format!("SUM(D2:D{nn})"), "boldMoney"),
            XCell::fs(dollars(est.lab_total), format!("SUM(E2:E{nn})"), "boldMoney"),
            XCell::fs(dollars(est.total), format!("SUM(F2:F{nn})"), "boldMoney"),
        ]);
        out.push(Sheet { name: t("div.sheet"), rows: rows_d, cols: vec![8.0, 48.0, 8.0, 14.0, 14.0, 14.0], freeze: 1 });
    }
    out
}

/// The "Alternates" worksheet.
pub fn alternate_rows(base: &Estimate, alts: &[model::Alternate]) -> Vec<Vec<XCell>> {
    let mut rows = vec![
        ["export.alt.h", "export.areas.item", "export.areas.unit", "export.areas.qty", "export.alt.amount", "export.areas.notes"]
            .iter()
            .map(|k| XCell::ts(t(k), "head"))
            .collect::<Vec<_>>(),
    ];
    rows.push(vec![
        XCell::ts(t("export.alt.base"), "bold"),
        XCell::Empty,
        XCell::Empty,
        XCell::Empty,
        XCell::vs(dollars(base.total), "boldMoney"),
        XCell::t(if base.incomplete { t("x.incomplete") } else { String::new() }),
    ]);
    rows.push(Vec::new());
    for a in alts {
        let name = tf(
            "export.alt.name",
            &[("name", F::S(jv::to_string(a.area.get("name")))), ("kind", F::S(jv::lower(&t(&format!("alt.{}", a.alt)))))],
        );
        for r in &a.est.rows {
            let err = jv::truthy(r.get("error"));
            rows.push(vec![
                XCell::t(name.clone()),
                bold_name(r),
                xcell_of(r.get("unit")),
                if err { XCell::Empty } else { XCell::vs(rnum(r, "qty"), "qty") },
                if err { XCell::Empty } else { XCell::vs(dollars(rnum(r, "total")), "money") },
                XCell::t(row_note(r)),
            ]);
        }
        rows.push(vec![
            XCell::ts(name.clone(), "bold"),
            XCell::Empty,
            XCell::Empty,
            XCell::Empty,
            XCell::vs(dollars(a.amount), "boldMoney"),
            XCell::t(if a.alt == "add" { t("export.alt.add.note") } else { t("export.alt.deduct.note") }),
        ]);
        rows.push(Vec::new());
    }
    let adds = alts.iter().filter(|a| a.alt == "add").fold(0.0, |n, a| n + a.amount);
    rows.push(vec![
        XCell::ts(t("export.alt.withadds"), "bold"),
        XCell::Empty,
        XCell::Empty,
        XCell::Empty,
        XCell::vs(dollars(base.total + adds), "boldMoney"),
        XCell::Empty,
    ]);
    rows
}

/// The "By bid area" worksheet.
pub fn area_rows(state: &Value) -> Vec<Vec<XCell>> {
    let head = [
        "export.areas.area",
        "export.areas.item",
        "export.areas.unit",
        "export.areas.qty",
        "export.areas.qtyw",
        "export.areas.mat",
        "export.areas.lab",
        "export.areas.total",
        "export.areas.notes",
    ];
    let mut rows = vec![head.iter().map(|k| XCell::ts(t(k), "head")).collect::<Vec<_>>()];
    let mut subs: Vec<(usize, Estimate)> = Vec::new();
    for (area, est) in area_estimates(state) {
        let name = match &area {
            Some(a) if jv::truthy(a.get("alt")) => tf(
                "export.alt.name",
                &[
                    ("name", F::S(jv::to_string(a.get("name")))),
                    ("kind", F::S(jv::lower(&t(&format!("alt.{}", jv::to_string(a.get("alt"))))))),
                ],
            ),
            Some(a) => jv::to_string(a.get("name")),
            None => t("export.areas.none"),
        };
        let first = rows.len().saturating_add(1);
        for r in &est.rows {
            if jv::truthy(r.get("error")) {
                rows.push(vec![
                    XCell::t(name.clone()),
                    bold_name(r),
                    xcell_of(r.get("unit")),
                    XCell::Empty,
                    XCell::Empty,
                    XCell::Empty,
                    XCell::Empty,
                    XCell::Empty,
                    XCell::t(row_note(r)),
                ]);
                continue;
            }
            rows.push(vec![
                XCell::t(name.clone()),
                bold_name(r),
                xcell_of(r.get("unit")),
                XCell::vs(rnum(r, "qty"), "qty"),
                XCell::vs(rnum(r, "qtyW"), "qty"),
                XCell::vs(dollars(rnum(r, "matExt")), "money"),
                XCell::vs(dollars(rnum(r, "labExt")), "money"),
                XCell::vs(dollars(rnum(r, "total")), "money"),
                XCell::t(row_note(r)),
            ]);
        }
        let last = rows.len();
        rows.push(vec![
            XCell::ts(tf("export.areas.subtotal", &[("area", F::S(name.clone()))]), "bold"),
            XCell::Empty,
            XCell::Empty,
            XCell::Empty,
            XCell::Empty,
            XCell::fs(dollars(est.mat_total), format!("SUM(F{first}:F{last})"), "boldMoney"),
            XCell::fs(dollars(est.lab_total), format!("SUM(G{first}:G{last})"), "boldMoney"),
            XCell::fs(dollars(est.total), format!("SUM(H{first}:H{last})"), "boldMoney"),
            XCell::t(if est.incomplete { t("export.areas.incomplete") } else { String::new() }),
        ]);
        subs.push((rows.len(), est));
        rows.push(Vec::new());
    }
    if !subs.is_empty() {
        let sum = |col: &str| subs.iter().map(|(r, _)| format!("{col}{r}")).collect::<Vec<_>>().join("+");
        let tot = |f: &dyn Fn(&Estimate) -> f64| subs.iter().fold(0.0, |n, (_, e)| n + f(e));
        rows.push(vec![
            XCell::ts(t("export.areas.all"), "bold"),
            XCell::Empty,
            XCell::Empty,
            XCell::Empty,
            XCell::Empty,
            XCell::fs(dollars(tot(&|e| e.mat_total)), sum("F"), "boldMoney"),
            XCell::fs(dollars(tot(&|e| e.lab_total)), sum("G"), "boldMoney"),
            XCell::fs(dollars(tot(&|e| e.total)), sum("H"), "boldMoney"),
            XCell::t(t("export.areas.all.note")),
        ]);
    }
    rows
}

/// The workbook (`estimateXlsx`).
pub fn estimate_xlsx(state: &Value, label: Labeler<'_>, compress: bool, date: &crate::xlsx::Stamp, local: &crate::xlsx::Stamp) -> Vec<u8> {
    let est = base_estimate(state);
    let sheets = estimate_sheets(state, label, &est);
    let title = jv::to_string(state.get("name"));
    crate::xlsx::write_xlsx(&sheets, &title, compress, date, local)
}

/// The cell types are shared with the harness output.
pub fn sheets_json(sheets: &[Sheet]) -> Value {
    Value::Array(
        sheets
            .iter()
            .map(|s| {
                serde_json::json!({
                    "name": s.name,
                    "rows": s.rows.iter().map(|r| Value::Array(r.iter().map(XCell::to_json).collect())).collect::<Vec<_>>(),
                    "cols": s.cols.iter().map(|c| jv::n(*c)).collect::<Vec<_>>(),
                    "freeze": s.freeze,
                })
            })
            .collect(),
    )
}

#[allow(dead_code)]
fn _cellv(_: CellV) {}
