//! ifc-qto.js — quantity totals grouped by properties and their CSV (0.4 B20), schedules one row per element or per
//! space (0.4 B10), and the CSV cell rules (formula-looking text gets a leading ').

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use cc_io_ifc::model::{Id, id_key};
use serde_json::{Map, Value as J};

use crate::js::{OMap, PV, hypot3, jn, locale_cmp, num_str, str_num, tidy, to_fixed};
use crate::model::Batch;
use crate::props::Index;

pub const MAX_GROUP: usize = 3;

/// A table cell / group value as the JS holds it.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Undef,
    Num(f64),
    Str(String),
    /// any other value: its `String(v)`
    Other(String),
}

impl Cell {
    pub fn of(v: Option<&PV>) -> Cell {
        match v {
            None => Cell::Str(String::new()),
            Some(PV::Num(n)) => Cell::Num(*n),
            Some(PV::Str(s)) => Cell::Str(s.to_string()),
        }
    }
    pub fn from_json(v: Option<&J>) -> Cell {
        match v {
            None | Some(J::Null) => Cell::Undef,
            Some(J::Number(n)) => Cell::Num(n.as_f64().unwrap_or(f64::NAN)),
            Some(J::String(s)) => Cell::Str(s.clone()),
            Some(other) => Cell::Other(cc_cad::js::to_js_string(other)),
        }
    }
    /// `String(v ?? '')`
    pub fn text(&self) -> String {
        match self {
            Cell::Undef => String::new(),
            Cell::Num(n) => num_str(*n),
            Cell::Str(s) | Cell::Other(s) => s.clone(),
        }
    }
    pub fn json(&self) -> J {
        match self {
            Cell::Undef => J::Null,
            Cell::Num(n) => jn(*n),
            Cell::Str(s) | Cell::Other(s) => J::String(s.clone()),
        }
    }
    fn is_empty_str(&self) -> bool {
        matches!(self, Cell::Str(s) if s.is_empty())
    }
}

/// `cmp(a, b)`: equal first, '' last, numbers as numbers, else localeCompare (numeric).
pub fn cmp(a: &Cell, b: &Cell) -> Ordering {
    let strict_eq = match (a, b) {
        (Cell::Num(x), Cell::Num(y)) => x == y,
        (Cell::Str(x), Cell::Str(y)) => x == y,
        (Cell::Undef, Cell::Undef) => true,
        _ => false,
    };
    if strict_eq {
        return Ordering::Equal;
    }
    if a.is_empty_str() {
        return Ordering::Greater;
    }
    if b.is_empty_str() {
        return Ordering::Less;
    }
    if let (Cell::Num(x), Cell::Num(y)) = (a, b) {
        return crate::js::sign_ord(x - y);
    }
    locale_cmp(&a.text(), &b.text())
}

fn cmp_rows(a: &[Cell], b: &[Cell]) -> Ordering {
    for (i, x) in a.iter().enumerate() {
        let c = cmp(x, b.get(i).unwrap_or(&Cell::Undef));
        if c.is_ne() {
            return c;
        }
    }
    Ordering::Equal
}

/// One totals row.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TRow {
    pub values: Vec<Cell>,
    pub count: f64,
    pub sums: Vec<f64>,
    pub counted: Vec<f64>,
}

impl TRow {
    fn json(&self, with_values: bool) -> J {
        let mut o = Map::new();
        if with_values {
            o.insert("values".into(), J::Array(self.values.iter().map(Cell::json).collect()));
        }
        o.insert("count".into(), jn(self.count));
        o.insert("sums".into(), J::Array(self.sums.iter().map(|x| jn(*x)).collect()));
        o.insert("counted".into(), J::Array(self.counted.iter().map(|x| jn(*x)).collect()));
        J::Object(o)
    }
    fn from_json(v: &J) -> TRow {
        let nums = |k: &str| -> Vec<f64> {
            v.get(k).and_then(J::as_array).map(|a| a.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect()).unwrap_or_default()
        };
        TRow {
            values: v.get("values").and_then(J::as_array).map(|a| a.iter().map(|x| Cell::from_json(Some(x))).collect()).unwrap_or_default(),
            count: v.get("count").and_then(J::as_f64).unwrap_or(0.0),
            sums: nums("sums"),
            counted: nums("counted"),
        }
    }
}

/// The totals result.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Totals {
    pub group_by: Vec<String>,
    pub sum: Vec<String>,
    pub rows: Vec<TRow>,
    pub total: TRow,
}

impl Totals {
    pub fn json(&self) -> J {
        let mut o = Map::new();
        o.insert("groupBy".into(), J::Array(self.group_by.iter().map(|s| J::String(s.clone())).collect()));
        o.insert("sum".into(), J::Array(self.sum.iter().map(|s| J::String(s.clone())).collect()));
        o.insert("rows".into(), J::Array(self.rows.iter().map(|r| r.json(true)).collect()));
        o.insert("total".into(), self.total.json(false));
        J::Object(o)
    }
    /// A result object handed to totalsCsv directly (the tests' hand-made ones).
    pub fn from_json(v: &J) -> Totals {
        let strs = |k: &str| -> Vec<String> {
            v.get(k).and_then(J::as_array).map(|a| a.iter().map(cc_cad::js::to_js_string).collect()).unwrap_or_default()
        };
        Totals {
            group_by: strs("groupBy"),
            sum: strs("sum"),
            rows: v.get("rows").and_then(J::as_array).map(|a| a.iter().map(TRow::from_json).collect()).unwrap_or_default(),
            total: v.get("total").map(TRow::from_json).unwrap_or_default(),
        }
    }
}

/// The ids a `keys` option names (deduplicated, only ids of the index), or every id.
fn ids_of(ix: &Index, keys: Option<&[f64]>) -> Vec<Id> {
    match keys {
        None => ix.ids.clone(),
        Some(keys) => {
            let known: HashSet<u64> = ix.ids.iter().map(|&i| id_key(i)).collect();
            let mut seen: HashSet<u64> = HashSet::new();
            let mut out = Vec::new();
            for k in keys {
                let id = k - ix.key_base;
                let ik = id_key(id);
                if seen.insert(ik) && known.contains(&ik) {
                    out.push(id);
                }
            }
            out
        }
    }
}

/// `totals(ix, { keys, groupBy, sum })`.
pub fn totals(ix: &Index, keys: Option<&[f64]>, group_by: &[String], sum: &[String]) -> Totals {
    let ids = ids_of(ix, keys);
    let gb: Vec<String> = group_by.iter().take(MAX_GROUP).cloned().collect();
    let gv: Vec<&crate::props::Vals> = gb.iter().map(|f| ix.values(f)).collect();
    let sv: Vec<&crate::props::Vals> = sum.iter().map(|f| ix.values(f)).collect();
    let zero = || vec![0.0; sum.len()];
    let mut rows: OMap<String, TRow> = OMap::new();
    let mut total = TRow { values: Vec::new(), count: 0.0, sums: zero(), counted: zero() };
    for id in ids {
        let k = id_key(id);
        let values: Vec<Cell> = gv.iter().map(|m| Cell::of(m.get(&k))).collect();
        // JSON.stringify(values): numbers by String(n) (−0 → 0), text quoted
        let key = values
            .iter()
            .map(|c| match c {
                Cell::Num(n) => format!("n{}", if n.is_finite() { num_str(*n) } else { "null".into() }),
                other => format!("s{}", other.text()),
            })
            .collect::<Vec<_>>()
            .join("\u{0}");
        let Some(r) = rows.entry(key, || TRow { values: values.clone(), count: 0.0, sums: zero(), counted: zero() }) else { continue };
        r.count += 1.0;
        total.count += 1.0;
        for (i, m) in sv.iter().enumerate() {
            if let Some(PV::Num(v)) = m.get(&k)
                && v.is_finite()
            {
                if let Some(s) = r.sums.get_mut(i) {
                    *s += v;
                }
                if let Some(c) = r.counted.get_mut(i) {
                    *c += 1.0;
                }
                if let Some(s) = total.sums.get_mut(i) {
                    *s += v;
                }
                if let Some(c) = total.counted.get_mut(i) {
                    *c += 1.0;
                }
            }
        }
    }
    let mut out = rows.vals;
    out.sort_by(|a, b| cmp_rows(&a.values, &b.values));
    for r in &mut out {
        r.sums = r.sums.iter().map(|x| tidy(*x)).collect();
    }
    total.sums = total.sums.iter().map(|x| tidy(*x)).collect();
    Totals { group_by: gb, sum: sum.to_vec(), rows: out, total }
}

/// `cell(v)`: a leading ' for formula-looking text (a negative number stays a number), quoted when needed.
pub fn csv_cell(s: &str) -> String {
    let mut s = s.to_string();
    let first = s.chars().next();
    if matches!(first, Some('=' | '+' | '-' | '@' | '\t' | '\r')) {
        let b = s.as_bytes();
        let digit_next = |i: usize| b.get(i).is_some_and(u8::is_ascii_digit);
        let negnum = digit_next(0) || (b.first() == Some(&b'-') && digit_next(1));
        if !negnum {
            s.insert(0, '\'');
        }
    }
    if s.contains(['"', ',', '\r', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s }
}

/// Lines of cells → CSV text (CRLF, a final CRLF).
pub fn csv(lines: &[Vec<Cell>]) -> String {
    let mut out =
        lines.iter().map(|l| l.iter().map(|c| csv_cell(&c.text())).collect::<Vec<_>>().join(",")).collect::<Vec<_>>().join("\r\n");
    out.push_str("\r\n");
    out
}

/// `tableCsv(head, rows)`.
pub fn table_csv(head: &[Cell], rows: &[Vec<Cell>]) -> String {
    let mut all = vec![head.to_vec()];
    all.extend(rows.iter().cloned());
    csv(&all)
}

/// The words totalsCsv takes (`label(f)`, countLabel, totalLabel, `leftOut(h)` = h + suffix).
#[derive(Clone, Debug, Default)]
pub struct CsvWords {
    pub labels: HashMap<String, String>,
    pub count: Option<String>,
    pub total: Option<String>,
    pub left_out_suffix: Option<String>,
}

impl CsvWords {
    fn label(&self, f: &str) -> String {
        self.labels.get(f).cloned().unwrap_or_else(|| f.to_string())
    }
    pub fn from_json(v: Option<&J>) -> CsvWords {
        let Some(v) = v else { return CsvWords::default() };
        let s = |k: &str| v.get(k).and_then(J::as_str).map(str::to_string);
        CsvWords {
            labels: v
                .get("labels")
                .and_then(J::as_object)
                .map(|o| o.iter().map(|(k, x)| (k.clone(), x.as_str().unwrap_or("").to_string())).collect())
                .unwrap_or_default(),
            count: s("countLabel"),
            total: s("totalLabel"),
            left_out_suffix: s("leftOutSuffix"),
        }
    }
}

/// `totalsCsv(res, words)`.
pub fn totals_csv(res: &Totals, w: &CsvWords) -> String {
    let gaps: Vec<bool> = (0..res.sum.len()).map(|i| res.total.counted.get(i).copied().unwrap_or(f64::NAN) < res.total.count).collect();
    let left = |h: String| format!("{h}{}", w.left_out_suffix.as_deref().unwrap_or(" (left out)"));
    let mut head: Vec<Cell> = res.group_by.iter().map(|f| Cell::Str(w.label(f))).collect();
    head.push(Cell::Str(w.count.clone().unwrap_or_else(|| "Count".into())));
    for (i, f) in res.sum.iter().enumerate() {
        head.push(Cell::Str(w.label(f)));
        if gaps.get(i).copied().unwrap_or(false) {
            head.push(Cell::Str(left(w.label(f))));
        }
    }
    let line = |values: &[Cell], r: &TRow| -> Vec<Cell> {
        let mut a = values.to_vec();
        a.push(Cell::Num(r.count));
        for i in 0..res.sum.len() {
            a.push(r.sums.get(i).map_or(Cell::Undef, |x| Cell::Num(*x)));
            if gaps.get(i).copied().unwrap_or(false) {
                a.push(Cell::Num(r.count - r.counted.get(i).copied().unwrap_or(f64::NAN)));
            }
        }
        a
    };
    let mut lines = vec![head];
    for r in &res.rows {
        lines.push(line(&r.values, r));
    }
    let mut tv = vec![Cell::Str(w.total.clone().unwrap_or_else(|| "Total".into()))];
    tv.extend(res.group_by.iter().skip(1).map(|_| Cell::Str(String::new())));
    lines.push(line(&tv, &res.total));
    csv(&lines)
}

/// `elementRows(ix, { keys, columns })` → (columns, rows of (key, values)).
pub fn element_rows(ix: &Index, keys: Option<&[f64]>, columns: &[String]) -> Vec<(f64, Vec<Cell>)> {
    let ids = ids_of(ix, keys);
    let cols: Vec<&crate::props::Vals> = columns.iter().map(|f| ix.values(f)).collect();
    let mut rows: Vec<(f64, Vec<Cell>)> =
        ids.iter().map(|&id| (ix.key_base + id, cols.iter().map(|m| Cell::of(m.get(&id_key(id)))).collect())).collect();
    rows.sort_by(|a, b| cmp_rows(&a.1, &b.1).then_with(|| crate::js::sign_ord(a.0 - b.0)));
    rows
}

/// `measureShapes(batches, want)`: floor area (downward faces, projected) and volume of each key's triangles.
fn measure_shapes(batches: &[Batch], want: &HashSet<u64>) -> HashMap<u64, (f64, f64)> {
    let mut out: OMap<u64, (f64, f64)> = OMap::new();
    for b in batches {
        let (e, p, ix) = (&b.elems, &b.positions, &b.indices);
        let pf = |i: f64| -> f64 { crate::clash::at_f32(p, i) };
        let ii = |t: usize| -> f64 { ix.get(t).map_or(f64::NAN, |&v| f64::from(v)) };
        for (r, &k) in e.ids.iter().enumerate() {
            let kk = id_key(f64::from(k));
            if !want.contains(&kk) {
                continue;
            }
            let Some(m) = out.entry(kk, || (0.0, 0.0)) else { continue };
            let t0 = e.t0.get(r).copied().unwrap_or(0);
            let tn = e.tn.get(r).copied().unwrap_or(0);
            for t in t0..t0.saturating_add(tn) {
                let t = usize::try_from(t).unwrap_or(usize::MAX);
                let a = ii(t.saturating_mul(3)) * 3.0;
                let c1 = ii(t.saturating_mul(3).saturating_add(1)) * 3.0;
                let c2 = ii(t.saturating_mul(3).saturating_add(2)) * 3.0;
                let (ux, uy, uz) = (pf(c1) - pf(a), pf(c1 + 1.0) - pf(a + 1.0), pf(c1 + 2.0) - pf(a + 2.0));
                let (vx, vy, vz) = (pf(c2) - pf(a), pf(c2 + 1.0) - pf(a + 1.0), pf(c2 + 2.0) - pf(a + 2.0));
                let ny = uz * vx - ux * vz;
                let len = hypot3(uy * vz - uz * vy, ny, ux * vy - uy * vx);
                if len > 0.0 && ny / len < -0.5 {
                    m.0 += -ny / 2.0;
                }
                m.1 += (pf(a) * (pf(c1 + 1.0) * pf(c2 + 2.0) - pf(c1 + 2.0) * pf(c2 + 1.0))
                    - pf(a + 1.0) * (pf(c1) * pf(c2 + 2.0) - pf(c1 + 2.0) * pf(c2))
                    + pf(a + 2.0) * (pf(c1) * pf(c2 + 1.0) - pf(c1 + 1.0) * pf(c2)))
                    / 6.0;
            }
        }
    }
    out.keys.iter().copied().zip(out.vals.iter().map(|(a, v)| (*a, v.abs()))).collect()
}

/// One row of the spaces schedule.
#[derive(Clone, Debug, PartialEq)]
pub struct SpaceRow {
    pub key: f64,
    pub number: Cell,
    pub name: Cell,
    pub storey: Cell,
    pub area: Cell,
    pub area_from: &'static str,
    pub volume: Cell,
    pub volume_from: &'static str,
}

/// `/re/.test(id)` for the two field patterns: `\.(Net|Gross)<end>$`.
fn net_gross(id: &str, end: &str) -> bool {
    id.strip_suffix(end).is_some_and(|s| s.ends_with(".Net") || s.ends_with(".Gross"))
}

/// `spaceSchedule(ix, model, { keys })` → (rows, count, area total, volume total).
pub fn space_schedule(ix: &Index, batches: &[Batch], keys: Option<&[f64]>) -> (Vec<SpaceRow>, usize, f64, f64) {
    let kb = ix.key_base;
    let ids: Vec<Id> =
        ids_of(ix, keys).into_iter().filter(|&id| matches!(ix.value("cls", id), Some(PV::Str(s)) if &**s == "IfcSpace")).collect();
    let find = |end: &str| -> Vec<&crate::props::Field> {
        let mut fs: Vec<&crate::props::Field> = ix.fields.iter().filter(|f| f.number && net_gross(&f.id, end)).collect();
        fs.sort_by(|a, b| {
            let n = |f: &crate::props::Field| u8::from(f.id.contains(".Net"));
            n(b).cmp(&n(a))
        });
        fs
    };
    let area_f = find("FloorArea");
    let vol_f = find("Volume");
    let want: HashSet<u64> = ids.iter().map(|id| id_key(kb + id)).collect();
    let shapes = measure_shapes(batches, &want);
    let first = |fs: &[&crate::props::Field], id: Id| -> Option<f64> {
        fs.iter().find_map(|f| match ix.value(&f.id, id) {
            Some(PV::Num(n)) => Some(*n),
            _ => None,
        })
    };
    let mut rows: Vec<SpaceRow> = ids
        .iter()
        .map(|&id| {
            let sh = shapes.get(&id_key(kb + id));
            let (a, v) = (first(&area_f, id), first(&vol_f, id));
            let fixed4 = |x: f64| Cell::Num(str_num(&to_fixed(x, 4)));
            SpaceRow {
                key: kb + id,
                number: ix.value("name", id).map_or(Cell::Str(String::new()), |p| Cell::of(Some(p))),
                name: ix.value("longname", id).map_or(Cell::Str(String::new()), |p| Cell::of(Some(p))),
                storey: ix.value("storey", id).map_or(Cell::Str(String::new()), |p| Cell::of(Some(p))),
                area: match (a, sh) {
                    (Some(a), _) => Cell::Num(a),
                    (None, Some(s)) => fixed4(s.0),
                    _ => Cell::Str(String::new()),
                },
                area_from: if a.is_some() {
                    "file"
                } else if sh.is_some() {
                    "shape"
                } else {
                    ""
                },
                volume: match (v, sh) {
                    (Some(v), _) => Cell::Num(v),
                    (None, Some(s)) => fixed4(s.1),
                    _ => Cell::Str(String::new()),
                },
                volume_from: if v.is_some() {
                    "file"
                } else if sh.is_some() {
                    "shape"
                } else {
                    ""
                },
            }
        })
        .collect();
    rows.sort_by(|x, y| cmp(&x.storey, &y.storey).then_with(|| cmp(&x.number, &y.number)));
    let sum = |f: fn(&SpaceRow) -> &Cell| tidy(rows.iter().fold(0.0, |s, r| s + if let Cell::Num(n) = f(r) { *n } else { 0.0 }));
    let (area, volume) = (sum(|r| &r.area), sum(|r| &r.volume));
    let n = rows.len();
    (rows, n, area, volume)
}

impl SpaceRow {
    pub fn json(&self) -> J {
        let mut o = Map::new();
        o.insert("key".into(), jn(self.key));
        o.insert("number".into(), self.number.json());
        o.insert("name".into(), self.name.json());
        o.insert("storey".into(), self.storey.json());
        o.insert("area".into(), self.area.json());
        o.insert("areaFrom".into(), J::String(self.area_from.into()));
        o.insert("volume".into(), self.volume.json());
        o.insert("volumeFrom".into(), J::String(self.volume_from.into()));
        J::Object(o)
    }
}

/// `quantityFields(ix)`: number fields, quantity sets (Qto_…, BaseQuantities) first.
pub fn quantity_fields(ix: &Index) -> Vec<&crate::props::Field> {
    let q = |f: &crate::props::Field| -> u8 {
        let g = f.group.as_ref().map_or_else(|| "undefined".to_string(), PV::text);
        let gl = g.to_lowercase();
        u8::from(!(gl.starts_with("qto_") || gl.contains("basequantities")))
    };
    let mut fs: Vec<&crate::props::Field> = ix.fields.iter().filter(|f| f.number).collect();
    fs.sort_by_key(|f| q(f));
    fs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells() {
        assert_eq!(csv_cell("=HYPERLINK(\"x\")"), "\"'=HYPERLINK(\"\"x\"\")\"");
        assert_eq!(csv_cell("-5"), "-5");
        assert_eq!(csv_cell("x,y"), "\"x,y\"");
        let t = table_csv(
            &[Cell::Str("A".into()), Cell::Str("B".into())],
            &[vec![Cell::Str("=1+1".into()), Cell::Num(2.0)], vec![Cell::Str("x,y".into()), Cell::Num(-3.0)]],
        );
        assert_eq!(t, "A,B\r\n'=1+1,2\r\n\"x,y\",-3\r\n");
    }
}
