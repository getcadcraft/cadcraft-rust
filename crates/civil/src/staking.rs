//! staking.js — staking reports (0.4 V23): stakes along an alignment at stations and offsets, or survey points
//! measured against it; the CSV. (The PDF table pages are in cc-io-civil with plot.js.)

use crate::alignment::{Alignment, Element};
use crate::js::{J, JsMath, Obj, max, min, replace, to_fixed};
use crate::points::Pt;
use crate::units::{fixed, format_station};
use crate::{re, tr};
use serde_json::Value;

const EPS: f64 = 1e-9;

/// `stakeStations(al, { from, to, step, keys })` (NaN from/to = the alignment's ends).
pub fn stake_stations(al: &Alignment, from: f64, to: f64, step: f64, keys: bool) -> Vec<f64> {
    let from = if from.is_nan() { al.start } else { from };
    let to = if to.is_nan() { al.end } else { to };
    let a = max(al.start, min(from, to));
    let b = min(al.end, max(from, to));
    let mut set: Vec<f64> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut add = |x: f64, set: &mut Vec<f64>| {
        let k = if x == 0.0 {
            0
        } else if x.is_nan() {
            1
        } else {
            x.to_bits()
        };
        if seen.insert(k) {
            set.push(x);
        }
    };
    add(a, &mut set);
    add(b, &mut set);
    if step > 0.0 {
        let mut k = ((a - EPS) / step).ceil();
        let mut guard = 0u64;
        while k * step <= b + EPS && guard < 10_000_000 {
            add(min(b, max(a, k * step)), &mut set);
            k += 1.0;
            guard = guard.saturating_add(1);
        }
    }
    if keys {
        for e in &al.elements {
            for s in [e.sta(), e.sta() + e.len()] {
                if s > a + EPS && s < b - EPS {
                    add(s, &mut set);
                }
            }
        }
    }
    crate::js::sort_by(&mut set, |x, y| x - y);
    let mut out: Vec<f64> = Vec::new();
    for (i, &s) in set.iter().enumerate() {
        if i == 0 || set.get(i - 1).is_some_and(|&p| s - p > 1e-6) {
            out.push(s);
        }
    }
    out
}

fn place(al: &Alignment, sta: f64, off: f64) -> (f64, f64) {
    match al.point_at(sta) {
        Some(p) => (p.x + off * p.dir.jsin(), p.y - off * p.dir.jcos()),
        None => (f64::NAN, f64::NAN),
    }
}

/// A staking row.
#[derive(Clone, Debug)]
pub struct Row {
    pub id: String,
    pub sta: f64,
    pub off: f64,
    pub x: f64,
    pub y: f64,
    pub design: Option<f64>,
    pub ground: Option<f64>,
    pub cf: Option<f64>,
}
impl J for Row {
    fn j(&self) -> Value {
        crate::jo! { "id" => self.id, "sta" => self.sta, "off" => self.off, "x" => self.x, "y" => self.y, "design" => self.design, "ground" => self.ground, "cf" => self.cf }
    }
}
pub fn read_row(v: &Value) -> Row {
    let o = |k: &str| v.get(k).filter(|x| !x.is_null()).map(|x| crate::js::numf(Some(x)));
    Row {
        id: crate::js::string_or_empty(v.get("id")),
        sta: crate::js::numf(v.get("sta")),
        off: crate::js::numf(v.get("off")),
        x: crate::js::numf(v.get("x")),
        y: crate::js::numf(v.get("y")),
        design: o("design"),
        ground: o("ground"),
        cf: o("cf"),
    }
}

fn finite(v: Option<f64>) -> Option<f64> {
    v.filter(|x| x.is_finite())
}

/// A design callback (sta, off, x, y) → z.
pub type DesignFn<'a> = &'a dyn Fn(f64, f64, f64, f64) -> Option<f64>;
/// A ground callback (x, y) → z.
pub type GroundFn<'a> = &'a dyn Fn(f64, f64) -> Option<f64>;

/// `stakeRows(al, { stations, offsets, design, ground, first, prefix })`.
pub fn stake_rows(
    al: &Alignment,
    stations: &[f64],
    offsets: &[f64],
    design: DesignFn,
    ground: GroundFn,
    first: f64,
    prefix: &str,
) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut n = first;
    for &sta in stations {
        for &off in offsets {
            let (x, y) = place(al, sta, off);
            let d = finite(design(sta, off, x, y));
            let g = finite(ground(x, y));
            let cf = match (d, g) {
                (Some(d), Some(g)) => Some(g - d),
                _ => None,
            };
            rows.push(Row { id: format!("{prefix}{}", crate::js::num_to_string(n)), sta, off, x, y, design: d, ground: g, cf });
            n += 1.0;
        }
    }
    rows
}

/// `pointRows(al, points, { design, ground })` → (rows, outside).
pub fn point_rows(al: &Alignment, points: &[Pt], design: DesignFn, ground: Option<GroundFn>) -> (Vec<Row>, usize) {
    let mut rows = Vec::new();
    let mut outside = 0usize;
    for p in points {
        let Some(so) = al.station_offset(p.x, p.y).filter(|so| !so.outside) else {
            outside = outside.saturating_add(1);
            continue;
        };
        let d = finite(design(so.sta, so.off, p.x, p.y));
        let g = match p.z.filter(|z| z.is_finite()) {
            Some(z) => Some(z),
            None => ground.and_then(|g| g(p.x, p.y)),
        };
        let g = finite(g);
        let cf = match (d, g) {
            (Some(d), Some(g)) => Some(g - d),
            _ => None,
        };
        rows.push(Row { id: p.n.clone(), sta: so.sta, off: so.off, x: p.x, y: p.y, design: d, ground: g, cf });
    }
    crate::js::sort_by(&mut rows, |a, b| {
        let d = a.sta - b.sta;
        if d != 0.0 && !d.is_nan() { d } else { a.off - b.off }
    });
    (rows, outside)
}

/// `parseOffsets(text)` → offsets (left −) or Err(token).
pub fn parse_offsets(text: &str) -> Result<Vec<f64>, String> {
    let u = crate::js::upper(text);
    let s1 = replace(re!(r"\bCL\b"), &u, "0", true);
    // .replace(/(^|[\s,;])([LR])\s+(\d)/g, '$1$2$3').replace(/(\d)\s+([LR])\b/g, '$1$2')
    let s2 = re!(r"(^|[\s,;])([LR])\s+(\d)").map_or(s1.clone(), |r| r.replace_all(&s1, "$1$2$3").into_owned());
    let src = re!(r"(\d)\s+([LR])\b").map_or(s2.clone(), |r| r.replace_all(&s2, "$1$2").into_owned());
    let mut out: Vec<f64> = Vec::new();
    for tok in src.split(|c: char| crate::js::is_ws(c) || c == ',' || c == ';').filter(|t| !t.is_empty()) {
        let Some(m) = crate::js::exec(re!(r"^([LR])?(-?\d+(?:\.\d+)?)([LR])?$"), tok) else { return Err(tok.to_string()) };
        let g = |i: usize| m.get(i).cloned().flatten();
        if g(1).is_some() && g(3).is_some() {
            return Err(tok.to_string());
        }
        let mut v = crate::js::str_to_number(&g(2).unwrap_or_default());
        if let Some(side) = g(1).or(g(3)) {
            if v < 0.0 {
                return Err(tok.to_string());
            }
            v = if side == "L" { -v } else { v };
        }
        if v.abs() > 1e5 {
            return Err(tok.to_string());
        }
        if !out.iter().any(|o| (o - v).abs() < 1e-9) {
            out.push(v);
        }
    }
    if out.len() > 200 { Err("…".into()) } else { Ok(out) }
}

/// `cutFillText(cf)`.
pub fn cut_fill_text(cf: Option<f64>) -> String {
    let Some(cf) = finite(cf) else { return String::new() };
    if cf.abs() < 0.005 {
        return "0.00".into();
    }
    format!("{}{}", if cf > 0.0 { "C " } else { "F " }, fixed(cf.abs(), 2).replace(',', ""))
}
/// `offsetText(off)`.
pub fn offset_text(off: f64) -> String {
    if off.abs() < 0.005 {
        return "0.00".into();
    }
    format!("{}{}", fixed(off.abs(), 2).replace(',', ""), if off < 0.0 { " L" } else { " R" })
}
fn num(v: Option<f64>, d: usize) -> String {
    finite(v).map_or(String::new(), |x| to_fixed(x, d))
}

/// `stakingCsv(rows, { units, title })`.
pub fn staking_csv(rows: &[Row], units: &str, title: &str) -> String {
    let u = if units == "m" { "m" } else { "ft" };
    let q = |s: String| -> String {
        let b = s.as_bytes();
        let numeric = b.first().is_some_and(u8::is_ascii_digit) || (b.first() == Some(&b'-') && b.get(1).is_some_and(u8::is_ascii_digit));
        if numeric || s.is_empty() {
            s
        } else if matches!(b.first(), Some(b'=' | b'+' | b'-' | b'@' | b'\t' | b'\r')) {
            format!("'{s}")
        } else {
            s
        }
    };
    let c = |v: String| if v.contains(['"', ',', '\r', '\n']) { format!("\"{}\"", v.replace('"', "\"\"")) } else { v };
    let mut out: Vec<Vec<String>> = Vec::new();
    if !title.is_empty() {
        out.push(vec![title.to_string()]);
    }
    out.push(vec![
        tr!("sk.c.point"),
        tr!("sk.c.sta"),
        tr!("sk.c.off", u = u),
        tr!("sk.c.n"),
        tr!("sk.c.e"),
        tr!("sk.c.design"),
        tr!("sk.c.ground"),
        tr!("sk.c.cf", u = u),
        tr!("sk.c.cfn"),
    ]);
    for r in rows {
        out.push(vec![
            r.id.clone(),
            format_station(r.sta, units, None),
            offset_text(r.off),
            num(Some(r.y), 3),
            num(Some(r.x), 3),
            num(r.design, 3),
            num(r.ground, 3),
            cut_fill_text(r.cf),
            num(r.cf, 3),
        ]);
    }
    let lines: Vec<String> = out.into_iter().map(|r| r.into_iter().map(|v| c(q(v))).collect::<Vec<_>>().join(",")).collect();
    format!("{}\r\n", lines.join("\r\n"))
}

/// Elements' (sta, len) for callers that need the key stations.
pub fn element_spans(al: &Alignment) -> Vec<(f64, f64)> {
    al.elements.iter().map(|e: &Element| (e.sta(), e.len())).collect()
}
/// JSON object of a parseOffsets result.
pub fn offsets_json(r: &Result<Vec<f64>, String>) -> Value {
    match r {
        Ok(v) => v.j(),
        Err(tok) => Obj::new().set("error", tok).done(),
    }
}
