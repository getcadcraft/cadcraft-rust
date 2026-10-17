//! csv.js — CSV exports of the markups list and the punch list (BOM + CRLF; cells starting with = + - @ tab or CR
//! get a leading ' so a spreadsheet never runs them as formulas).

use crate::i18n::t0;
use crate::js::ns;
use crate::model::{Markup, punch_status_name, status_name, type_name};

/// A cell value: text or a number (`String(v)` when written).
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    S(String),
    N(f64),
}

impl Cell {
    fn text(&self) -> String {
        match self {
            Cell::S(s) => s.clone(),
            Cell::N(n) => ns(*n),
        }
    }
}

impl From<&str> for Cell {
    fn from(s: &str) -> Self {
        Cell::S(s.to_string())
    }
}
impl From<String> for Cell {
    fn from(s: String) -> Self {
        Cell::S(s)
    }
}

/// `cell(v)`.
pub fn cell(v: &Cell) -> String {
    let mut s = v.text();
    if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        s.insert(0, '\'');
    }
    if s.contains(['"', ',', '\r', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s }
}

/// `toCsv(rows)`.
pub fn to_csv(rows: &[Vec<Cell>]) -> String {
    let body: Vec<String> = rows.iter().map(|r| r.iter().map(cell).collect::<Vec<_>>().join(",")).collect();
    format!("\u{feff}{}\r\n", body.join("\r\n"))
}

fn head(keys: &[&str]) -> Vec<Cell> {
    keys.iter().map(|k| Cell::S(t0(&format!("csv.{k}")))).collect()
}

/// What `sheetOf(m)` answers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SheetRef {
    pub title: String,
    pub file: String,
    pub page: Option<Cell>,
}

fn last_reply(m: &Markup) -> String {
    match m.replies.last() {
        Some(r) => format!("{}{}", if r.author.is_empty() { String::new() } else { format!("{}: ", r.author) }, r.text),
        None => String::new(),
    }
}

/// A custom column (key, name) for the markups CSV.
#[derive(Debug, Clone, PartialEq)]
pub struct CsvColumn {
    pub key: String,
    pub name: String,
}

/// `markupRows(markups, sheetOf, measureOf, spaceOf, columns)`.
pub fn markup_rows(
    markups: &[&Markup],
    sheet_of: &dyn Fn(&Markup) -> Option<SheetRef>,
    measure_of: &dyn Fn(&Markup) -> String,
    space_of: Option<&dyn Fn(&Markup) -> String>,
    columns: &[CsvColumn],
) -> Vec<Vec<Cell>> {
    let mut keys = vec![
        "punch_no",
        "sheet",
        "file",
        "page",
        "type",
        "subject",
        "text",
        "comment",
        "measurement",
        "author",
        "created",
        "modified",
        "status",
        "replies",
        "last_reply",
        "color",
        "read_only",
    ];
    if space_of.is_some() {
        keys.push("space");
    }
    let mut first = head(&keys);
    first.extend(columns.iter().map(|c| Cell::S(c.name.clone())));
    let mut rows = vec![first];
    for m in markups {
        let s = sheet_of(m).unwrap_or_default();
        let ty = if m.kind == "foreign" {
            match m.src.as_ref().map(|s| s.subtype.clone()).filter(|s| !s.is_empty()) {
                Some(st) => st,
                None => type_name("foreign"),
            }
        } else if crate::model::is_type(&m.kind) {
            type_name(&m.kind)
        } else {
            m.kind.clone()
        };
        let text = if m.kind == "stamp" || m.kind == "image" || m.kind == "count" { m.label.clone() } else { m.text.clone() };
        #[allow(clippy::cast_precision_loss)]
        let mut row = vec![
            m.punch.as_ref().map_or(Cell::S(String::new()), |p| Cell::N(p.n)),
            Cell::S(s.title),
            Cell::S(s.file),
            s.page.unwrap_or(Cell::S(String::new())),
            Cell::S(ty),
            Cell::S(m.subject.clone()),
            Cell::S(text),
            Cell::S(m.comment.clone()),
            Cell::S(measure_of(m)),
            Cell::S(m.author.clone()),
            Cell::S(m.created.clone()),
            Cell::S(m.modified.clone()),
            Cell::S(status_name(&m.status)),
            Cell::N(m.replies.len() as f64),
            Cell::S(last_reply(m)),
            Cell::S(m.style.color.clone()),
            Cell::S(if m.locked { t0("csv.yes") } else { String::new() }),
        ];
        if let Some(sp) = space_of {
            row.push(Cell::S(sp(m)));
        }
        for c in columns {
            row.push(Cell::S(m.fields.get(&c.key).cloned().unwrap_or_default()));
        }
        rows.push(row);
    }
    rows
}

/// `punchRows(markups, sheetOf)`.
pub fn punch_rows(markups: &[&Markup], sheet_of: &dyn Fn(&Markup) -> Option<SheetRef>) -> Vec<Vec<Cell>> {
    let mut rows =
        vec![head(&["no", "sheet", "file", "page", "item", "comment", "assignee", "due", "status", "markup_status", "author", "created"])];
    let mut list: Vec<&Markup> = markups.iter().copied().filter(|m| m.punch.is_some()).collect();
    let n = |m: &Markup| m.punch.as_ref().map_or(f64::NAN, |p| p.n);
    cc_m3d::js::sort_by(&mut list, |a, b| n(a) - n(b));
    for m in list {
        let Some(p) = &m.punch else { continue };
        let s = sheet_of(m).unwrap_or_default();
        let item = if !m.subject.is_empty() {
            m.subject.clone()
        } else if crate::model::is_type(&m.kind) {
            type_name(&m.kind)
        } else {
            String::new()
        };
        let comment = if m.kind == "text" || m.kind == "callout" {
            [m.text.as_str(), m.comment.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" — ")
        } else {
            m.comment.clone()
        };
        let day: String = m.created.chars().take(10).collect();
        rows.push(vec![
            Cell::N(p.n),
            Cell::S(s.title),
            Cell::S(s.file),
            s.page.unwrap_or(Cell::S(String::new())),
            Cell::S(item),
            Cell::S(comment),
            Cell::S(p.assignee.clone()),
            Cell::S(p.due.clone()),
            Cell::S(punch_status_name(&p.status)),
            Cell::S(status_name(&m.status)),
            Cell::S(m.author.clone()),
            Cell::S(day),
        ]);
    }
    rows
}
