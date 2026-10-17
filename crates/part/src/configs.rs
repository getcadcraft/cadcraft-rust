//! configs.js — configurations: design-table rows saved in the part (`part.configs = { rows: [{ name, values }],
//! active }`): the untrusted reader, rows from the current values, save / apply / match / rename / delete, following a
//! parameter edit, rows from a CSV table and the CSV writer.

use crate::expr;
use crate::i18n::{F, t};
use crate::js;
use crate::jv;
use crate::params::{self, Params};
use serde_json::Value;
use std::collections::HashSet;

pub const CONFIG_MAX_ROWS: usize = 500;
pub const CONFIG_NAME_MAX: usize = 60;

/// `cleanName(s)`.
pub fn clean_name(v: Option<&Value>) -> String {
    js::slice16(js::trim(&js::strip_ctrl(&js::str_of(v))), CONFIG_NAME_MAX)
}
fn clean_str(s: &str) -> String {
    js::slice16(js::trim(&js::strip_ctrl(s)), CONFIG_NAME_MAX)
}

/// A row value: a number, or whatever else a caller handed `saveRow` (the JS copies any value through; only the
/// reader insists on finite numbers).
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Num(f64),
    Other(Value),
}
impl Cell {
    fn from_json(x: &Value) -> Cell {
        match x {
            Value::String(s) if s == "$NaN" => Cell::Num(f64::NAN),
            Value::String(s) if s == "$Inf" => Cell::Num(f64::INFINITY),
            Value::String(s) if s == "$-Inf" => Cell::Num(f64::NEG_INFINITY),
            _ => jv::as_num(Some(x)).map_or_else(|| Cell::Other(x.clone()), Cell::Num),
        }
    }
    fn to_json(&self) -> Value {
        match self {
            Cell::Num(x) => jv::num(*x),
            Cell::Other(v) => v.clone(),
        }
    }
    /// `ToNumber(v)`.
    pub fn number(&self) -> f64 {
        match self {
            Cell::Num(x) => *x,
            Cell::Other(v) => to_number(v, 0),
        }
    }
    /// `formatNumber(v)`.
    fn format(&self) -> String {
        match self {
            Cell::Num(x) => expr::format_number(*x),
            Cell::Other(v) => js::string_of(Some(v)),
        }
    }
}
/// JS `ToNumber` of a JSON value (objects → NaN; a one-element array → its element's string).
fn to_number(v: &Value, depth: usize) -> f64 {
    match v {
        Value::Null => 0.0,
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::String(s) => string_to_number(s),
        Value::Array(a) if a.is_empty() => 0.0,
        Value::Array(a) if a.len() == 1 && depth < 32 => match a.first() {
            Some(x @ Value::Array(_)) => to_number(x, depth.saturating_add(1)),
            Some(Value::Null) => 0.0,
            Some(x) => string_to_number(&js::string_of(Some(x))),
            None => 0.0,
        },
        _ => f64::NAN,
    }
}
fn string_to_number(s: &str) -> f64 {
    let t = js::trim(s);
    if t.is_empty() {
        return 0.0;
    }
    let (sign, body) = match t.strip_prefix('-') {
        Some(b) => (-1.0, b),
        None => (1.0, t.strip_prefix('+').unwrap_or(t)),
    };
    if body == "Infinity" {
        return sign * f64::INFINITY;
    }
    if sign > 0.0 && t.len() > 2 {
        let radix = match t.get(..2) {
            Some("0x" | "0X") => 16,
            Some("0o" | "0O") => 8,
            Some("0b" | "0B") => 2,
            _ => 0,
        };
        if radix != 0 {
            let digits = t.get(2..).unwrap_or("");
            return digits
                .chars()
                .try_fold(0.0f64, |acc, c| c.to_digit(radix).map(|d| acc * f64::from(radix) + f64::from(d)))
                .unwrap_or(f64::NAN);
        }
    }
    let ok = !body.is_empty()
        && body.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
        && body.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '.');
    if !ok {
        return f64::NAN;
    }
    body.parse::<f64>().map_or(f64::NAN, |x| sign * x)
}

/// One row: name + values (parameter → value, insertion order).
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub name: String,
    pub values: Vec<(String, Cell)>,
}
/// `{ rows, active }`.
#[derive(Clone, Debug, PartialEq)]
pub struct Configs {
    pub rows: Vec<Row>,
    pub active: Option<String>,
}

impl Row {
    pub fn to_json(&self) -> Value {
        let mut vals = jv::Obj::new();
        for (k, v) in &self.values {
            vals.insert(k.clone(), v.to_json());
        }
        jv::obj(vec![("name", self.name.as_str().into()), ("values", Value::Object(vals))])
    }
    pub fn from_json(v: &Value) -> Row {
        let values =
            jv::as_obj(v.get("values")).map(|o| o.iter().map(|(k, x)| (k.clone(), Cell::from_json(x))).collect()).unwrap_or_default();
        Row { name: js::str_of(v.get("name")), values }
    }
    fn set(&mut self, k: &str, v: Cell) {
        match self.values.iter_mut().find(|(n, _)| n == k) {
            Some(s) => s.1 = v,
            None => self.values.push((k.to_string(), v)),
        }
    }
}
impl Configs {
    pub fn to_json(&self) -> Value {
        jv::obj(vec![
            ("rows", Value::Array(self.rows.iter().map(Row::to_json).collect())),
            ("active", self.active.as_deref().map_or(Value::Null, Into::into)),
        ])
    }
    pub fn from_json(v: &Value) -> Configs {
        Configs {
            rows: jv::arr(v.get("rows")).iter().map(Row::from_json).collect(),
            active: v.get("active").and_then(Value::as_str).map(str::to_string),
        }
    }
}

fn var_names(p: Option<&Params>) -> HashSet<String> {
    p.map(|p| p.vars.iter().map(|v| v.name.clone()).collect()).unwrap_or_default()
}

/// `readConfigs(raw, params)`: None when there are no rows.
pub fn read_configs(raw: Option<&Value>, p: Option<&Params>) -> Result<Option<Configs>, String> {
    let Some(raw) = raw.filter(|r| !r.is_null()) else { return Ok(None) };
    let rows_v = match raw.get("rows") {
        Some(Value::Array(a)) if jv::is_obj_like(Some(raw)) && a.len() <= CONFIG_MAX_ROWS => a,
        _ => return Err(t("configs.err.list", &[])),
    };
    let names = var_names(p);
    let mut seen: HashSet<String> = HashSet::new();
    let mut rows = Vec::new();
    for r in rows_v {
        let name = match r.get("name") {
            Some(Value::String(s)) if jv::truthy(Some(r)) => clean_str(s),
            _ => String::new(),
        };
        if name.is_empty() || seen.contains(&js::lower(&name)) {
            return Err(t("configs.err.name", &[]));
        }
        seen.insert(js::lower(&name));
        let Some(vals) = jv::as_obj(r.get("values")) else { return Err(t("configs.err.values", &[("name", F::S(name))])) };
        let mut values = Vec::new();
        for (k, v) in vals {
            if !names.contains(k) {
                return Err(t("configs.err.unknown", &[("name", F::S(name)), ("param", F::S(js::slice16(k, 40)))]));
            }
            match jv::finite(Some(v)) {
                Some(x) => values.push((k.clone(), Cell::Num(x))),
                None => return Err(t("configs.err.number", &[("name", F::S(name)), ("param", F::S(k.clone()))])),
            }
        }
        rows.push(Row { name, values });
    }
    if rows.is_empty() {
        return Ok(None);
    }
    let active = raw.get("active").and_then(Value::as_str).filter(|a| rows.iter().any(|r| r.name == *a)).map(str::to_string);
    Ok(Some(Configs { rows, active }))
}

/// `rowFromCurrent(params, name)`.
pub fn row_from_current(p: Option<&Params>, name: Option<&Value>) -> Row {
    let empty = Params::default();
    let pp = p.unwrap_or(&empty);
    let vals = params::evaluate_vars(pp, None).values;
    let mut values = Vec::new();
    for v in &pp.vars {
        if params::is_driving(v)
            && let Some(x) = vals.get(&v.name).filter(|x| x.is_finite())
        {
            match values.iter_mut().find(|(n, _): &&mut (String, Cell)| *n == v.name) {
                Some(s) => s.1 = Cell::Num(*x),
                None => values.push((v.name.clone(), Cell::Num(*x))),
            }
        }
    }
    Row { name: clean_name(name), values }
}

/// `saveRow(cfg, row)`.
pub fn save_row(cfg: Option<&Configs>, row: &Row) -> Result<Configs, String> {
    let mut out = cfg.cloned().unwrap_or(Configs { rows: Vec::new(), active: None });
    let ln = js::lower(&row.name);
    match out.rows.iter().position(|r| js::lower(&r.name) == ln) {
        Some(i) => {
            if let Some(r) = out.rows.get_mut(i) {
                r.values = row.values.clone();
            }
        }
        None => {
            if out.rows.len() >= CONFIG_MAX_ROWS {
                return Err(t("configs.err.many", &[("n", F::from(CONFIG_MAX_ROWS))]));
            }
            out.rows.push(row.clone());
        }
    }
    Ok(out)
}

/// `applyRow(params, row)`.
pub fn apply_row(p: &mut Params, row: &Row) -> Result<(), String> {
    for (k, v) in &row.values {
        let Some(w) = p.vars.iter_mut().find(|x| x.name == *k) else {
            return Err(t("configs.err.missing", &[("name", F::S(row.name.clone())), ("param", F::S(k.clone()))]));
        };
        w.expr = v.format();
    }
    Ok(())
}

/// `rowMatches(params, row)`.
pub fn row_matches(p: Option<&Params>, row: &Row) -> bool {
    let empty = Params::default();
    let vals = params::evaluate_vars(p.unwrap_or(&empty), None).values;
    row.values.iter().all(|(k, v)| {
        let v = v.number();
        vals.get(k).is_some_and(|c| c.is_finite() && (c - v).abs() <= 1e-12 * js::max(1.0, v.abs()))
    })
}

/// `renameRow(cfg, from, to)`.
pub fn rename_row(cfg: &Configs, from: &str, to: Option<&Value>) -> Result<Configs, String> {
    let name = clean_name(to);
    if name.is_empty() {
        return Err(t("configs.err.name", &[]));
    }
    let ln = js::lower(&name);
    if cfg.rows.iter().any(|r| r.name != from && js::lower(&r.name) == ln) {
        return Err(t("configs.err.taken", &[("name", F::S(name))]));
    }
    Ok(Configs {
        rows: cfg
            .rows
            .iter()
            .map(|r| Row { name: if r.name == from { name.clone() } else { r.name.clone() }, values: r.values.clone() })
            .collect(),
        active: if cfg.active.as_deref() == Some(from) { Some(name.clone()) } else { cfg.active.clone() },
    })
}

/// `deleteRow(cfg, name)`: None when the last row goes.
pub fn delete_row(cfg: &Configs, name: &str) -> Option<Configs> {
    let rows: Vec<Row> = cfg.rows.iter().filter(|r| r.name != name).cloned().collect();
    if rows.is_empty() {
        return None;
    }
    Some(Configs { rows, active: if cfg.active.as_deref() == Some(name) { None } else { cfg.active.clone() } })
}

/// `followParams(cfg, before, after)`.
pub fn follow_params(cfg: Option<&Configs>, before: Option<&Params>, after: Option<&Params>) -> Option<Configs> {
    let cfg = cfg?;
    let empty = Params::default();
    let (b, a) = (before.unwrap_or(&empty), after.unwrap_or(&empty));
    let mut ren: Vec<(String, String)> = Vec::new();
    if b.vars.len() == a.vars.len() {
        for (v, w) in b.vars.iter().zip(a.vars.iter()) {
            if v.name != w.name {
                match ren.iter_mut().find(|(k, _)| *k == v.name) {
                    Some(s) => s.1 = w.name.clone(),
                    None => ren.push((v.name.clone(), w.name.clone())),
                }
            }
        }
    }
    let names = var_names(Some(a));
    let rows = cfg
        .rows
        .iter()
        .map(|r| {
            let mut out = Row { name: r.name.clone(), values: Vec::new() };
            for (k, v) in &r.values {
                let n = ren.iter().find(|(x, _)| x == k).map_or_else(|| k.clone(), |(_, y)| y.clone());
                if names.contains(&n) {
                    out.set(&n, v.clone());
                }
            }
            out
        })
        .collect();
    Some(Configs { rows, active: cfg.active.clone() })
}

/// `rowsFromTable(csv, params)`.
pub fn rows_from_table(csv: &str, p: &Params) -> Result<Vec<Row>, String> {
    let (rows, _) = params::read_design_table(csv, p)??;
    Ok(rows
        .into_iter()
        .map(|r| {
            let mut row = Row { name: clean_str(&r.name), values: Vec::new() };
            for (k, v) in r.overrides.iter() {
                row.set(k, Cell::Num(*v));
            }
            row
        })
        .collect())
}

/// `configsCsv(cfg)`.
pub fn configs_csv(cfg: Option<&Configs>) -> String {
    let rows: &[Row] = cfg.map_or(&[], |c| c.rows.as_slice());
    let mut cols: Vec<String> = Vec::new();
    for r in rows {
        for (k, _) in &r.values {
            if !cols.contains(k) {
                cols.push(k.clone());
            }
        }
    }
    let cell = |s: &str| -> String {
        let mut x = s.to_string();
        let digit_start = {
            let b = x.as_bytes();
            let i = usize::from(b.first() == Some(&b'-'));
            b.get(i).is_some_and(u8::is_ascii_digit)
        };
        if x.starts_with(['=', '+', '-', '@']) && !digit_start {
            x.insert(0, '\'');
        }
        if x.contains(['"', ',', ';', '\r', '\n']) { format!("\"{}\"", x.replace('"', "\"\"")) } else { x }
    };
    let mut lines: Vec<Vec<String>> = vec![std::iter::once("name".to_string()).chain(cols.iter().cloned()).collect()];
    for r in rows {
        let mut l = vec![r.name.clone()];
        for c in &cols {
            l.push(r.values.iter().find(|(k, _)| k == c).map_or_else(String::new, |(_, v)| v.format()));
        }
        lines.push(l);
    }
    let body: Vec<String> = lines.iter().map(|l| l.iter().map(|s| cell(s)).collect::<Vec<_>>().join(",")).collect();
    format!("{}\r\n", body.join("\r\n"))
}
