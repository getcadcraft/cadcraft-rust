//! summary.js `summarySheets` — the markup summary as workbook sheets for [`crate::xlsx::write_xlsx`] (0.4 K6):
//! Markups (the CSV columns), By sheet (counts per sheet and type, live SUM totals) and the Punch list.
//! The PDF report (`summaryPdf`) is in `cc-io-markup`.

use crate::csv::{Cell, CsvColumn, SheetRef, markup_rows, punch_rows};
use crate::i18n::t0;
use crate::model::{Markup, is_type, type_name};
use crate::slip::num_value;
use crate::xlsx::col_name;
use serde_json::{Map, Value, json};

const CLOSED: [&str; 3] = ["Accepted", "Rejected", "Completed"];

fn cell_value(c: &Cell) -> Value {
    match c {
        Cell::S(s) => Value::String(s.clone()),
        Cell::N(n) => num_value(*n),
    }
}

fn styled(v: Value, s: &str) -> Value {
    let mut o = Map::new();
    o.insert("v".into(), v);
    o.insert("s".into(), Value::String(s.into()));
    Value::Object(o)
}

fn sheet(name: String, cols: Vec<f64>, rows: Vec<Value>) -> Value {
    json!({ "name": name, "cols": cols.into_iter().map(num_value).collect::<Vec<_>>(), "rows": rows, "freeze": 1 })
}

fn page_text(p: &Option<Cell>) -> String {
    match p {
        None => String::new(),
        Some(Cell::S(s)) => s.clone(),
        Some(Cell::N(n)) => crate::js::num_to_string(*n),
    }
}

/// `summarySheets(markups, {sheetOf, measureOf, spaceOf, columns})`.
pub fn summary_sheets(
    markups: &[&Markup],
    sheet_of: &dyn Fn(&Markup) -> Option<SheetRef>,
    measure_of: &dyn Fn(&Markup) -> String,
    space_of: Option<&dyn Fn(&Markup) -> String>,
    columns: &[CsvColumn],
) -> Vec<Value> {
    let rows = markup_rows(markups, sheet_of, measure_of, space_of, columns);
    let mut out_rows: Vec<Value> = Vec::new();
    if let Some(h) = rows.first() {
        out_rows.push(Value::Array(h.iter().map(|c| styled(cell_value(c), "head")).collect()));
    }
    for r in rows.iter().skip(1) {
        out_rows.push(Value::Array(
            r.iter()
                .enumerate()
                .map(|(i, c)| match c {
                    Cell::N(_) if i == 13 || i == 3 || i == 0 => styled(cell_value(c), "int"),
                    Cell::N(_) => cell_value(c),
                    Cell::S(s) if s.is_empty() => Value::Null,
                    Cell::S(s) => Value::String(s.clone()),
                })
                .collect(),
        ));
    }
    let mut sheets = vec![sheet(
        t0("sum.sheet.markups"),
        vec![8.0, 22.0, 22.0, 6.0, 14.0, 18.0, 36.0, 36.0, 18.0, 18.0, 20.0, 20.0, 12.0, 8.0, 36.0, 9.0, 9.0, 18.0],
        out_rows,
    )];
    // per sheet
    let mut types: Vec<String> = Vec::new();
    for m in markups {
        if !types.contains(&m.kind) {
            types.push(m.kind.clone());
        }
    }
    struct Per {
        s: SheetRef,
        n: f64,
        open: f64,
        closed: f64,
        by: Vec<(String, f64)>,
    }
    let mut per: Vec<(String, Per)> = Vec::new();
    for m in markups {
        let s = sheet_of(m).unwrap_or_default();
        let k = format!("{}\u{0}{}\u{0}{}", s.title, s.file, page_text(&s.page));
        let idx = match per.iter().position(|(kk, _)| *kk == k) {
            Some(i) => i,
            None => {
                per.push((k, Per { s, n: 0.0, open: 0.0, closed: 0.0, by: Vec::new() }));
                per.len().saturating_sub(1)
            }
        };
        let Some((_, e)) = per.get_mut(idx) else { continue };
        e.n += 1.0;
        if CLOSED.contains(&m.status.as_str()) {
            e.closed += 1.0;
        } else {
            e.open += 1.0;
        }
        match e.by.iter_mut().find(|(t, _)| *t == m.kind) {
            Some((_, c)) => *c += 1.0,
            None => e.by.push((m.kind.clone(), 1.0)),
        }
    }
    let type_label = |ty: &str| if is_type(ty) { type_name(ty) } else { ty.to_string() };
    let mut h2: Vec<Value> = ["csv.sheet", "csv.file", "csv.page", "sum.col.markups", "sum.col.open", "sum.col.closed"]
        .iter()
        .map(|k| styled(Value::String(t0(k)), "head"))
        .collect();
    h2.extend(types.iter().map(|ty| styled(Value::String(type_label(ty)), "head")));
    // the numbers of each row (for the totals) beside its cells
    let mut r2: Vec<(Vec<Value>, Vec<f64>)> = Vec::new();
    for (_, e) in &per {
        let mut cells = vec![
            Value::String(e.s.title.clone()),
            Value::String(e.s.file.clone()),
            e.s.page.as_ref().map_or(Value::Null, cell_value),
            styled(num_value(e.n), "int"),
            styled(num_value(e.open), "int"),
            styled(num_value(e.closed), "int"),
        ];
        let mut nums = vec![0.0, 0.0, 0.0, e.n, e.open, e.closed];
        for ty in &types {
            let c = e.by.iter().find(|(t, _)| t == ty).map_or(0.0, |(_, c)| *c);
            cells.push(if c != 0.0 { styled(num_value(c), "int") } else { Value::Null });
            nums.push(c);
        }
        r2.push((cells, nums));
    }
    let n = r2.len();
    let mut rows2: Vec<Value> = vec![Value::Array(h2)];
    rows2.extend(r2.iter().map(|(c, _)| Value::Array(c.clone())));
    if n > 0 {
        let total = |c: usize| -> Value {
            let v = r2.iter().map(|(_, nums)| nums.get(c).copied().unwrap_or(0.0)).fold(0.0, |a, b| a + b);
            let cn = col_name(c);
            json!({ "v": num_value(v), "f": format!("SUM({cn}2:{cn}{})", n.saturating_add(1)), "s": "int" })
        };
        let mut tot = vec![styled(Value::String(t0("sum.total")), "bold"), Value::Null, Value::Null];
        for c in [3usize, 4, 5] {
            tot.push(total(c));
        }
        for i in 0..types.len() {
            tot.push(total(i.saturating_add(6)));
        }
        rows2.push(Value::Array(tot));
    }
    let mut cols2 = vec![24.0, 24.0, 6.0, 9.0, 9.0, 9.0];
    cols2.extend(types.iter().map(|_| 12.0));
    sheets.push(sheet(t0("sum.sheet.by_sheet"), cols2, rows2));
    let pr = punch_rows(markups, sheet_of);
    if pr.len() > 1 {
        let mut rows3: Vec<Value> = Vec::new();
        if let Some(h) = pr.first() {
            rows3.push(Value::Array(h.iter().map(|c| styled(cell_value(c), "head")).collect()));
        }
        for r in pr.iter().skip(1) {
            rows3.push(Value::Array(
                r.iter()
                    .enumerate()
                    .map(|(i, c)| match c {
                        _ if i == 0 => styled(cell_value(c), "int"),
                        Cell::S(s) if s.is_empty() => Value::Null,
                        c => cell_value(c),
                    })
                    .collect(),
            ));
        }
        sheets.push(sheet(t0("sum.sheet.punch"), vec![6.0, 22.0, 22.0, 6.0, 18.0, 40.0, 18.0, 12.0, 14.0, 14.0, 18.0, 12.0], rows3));
    }
    sheets
}
