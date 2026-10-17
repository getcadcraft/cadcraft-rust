//! spaces.js — Spaces (0.4 K17): every markup on a sheet is "in" the smallest space holding the middle of its bounds.
//! sketch.js — sketch to scale (0.4 K18): the next point typed as a real length and an angle.
//! legend.js — a markup legend that counts as you go (0.4 K21).

use crate::collate::locale_compare;
use crate::holes::{in_ring, ring_area};
use crate::model::Markup;
use crate::units::{Cal, parse_length};

pub type P = [f64; 2];

/// `spaceAt(p, spaces)` → the index of the smallest space whose outline holds p.
pub fn space_at(p: P, spaces: &[&Markup]) -> Option<usize> {
    let mut best = None;
    let mut area = f64::INFINITY;
    for (i, s) in spaces.iter().enumerate() {
        let Some(pts) = s.pts.as_ref() else { continue };
        if pts.len() < 3 || !in_ring(p, pts) {
            continue;
        }
        let a = ring_area(pts);
        if a < area {
            best = Some(i);
            area = a;
        }
    }
    best
}

/// `spaceOf(m, spacesOnSheet, boundsOf)` → the space's name ('' when none; spaces are in none).
pub fn space_of(m: &Markup, spaces: &[&Markup], bounds_of: &dyn Fn(&Markup) -> Option<[f64; 4]>) -> String {
    if m.kind == "space" || spaces.is_empty() {
        return String::new();
    }
    let Some(b) = bounds_of(m) else { return String::new() };
    match space_at([(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0], spaces).and_then(|i| spaces.get(i)) {
        Some(s) => s.label.clone(),
        None => String::new(),
    }
}

/// `sketchPoint(from, toward, text, cal, rot)` → `Ok(pt)` or `Err(language key)`.
pub fn sketch_point(from: P, toward: P, text: &str, cal: Option<&Cal>, rot: f64) -> Result<P, &'static str> {
    let Some(cal) = cal.filter(|c| c.mm_per_pt > 0.0) else { return Err("sketch.err.scale") };
    let mut parts = text.split(['<', '@']);
    let len_text = parts.next().unwrap_or("");
    let ang_text = parts.next();
    let mm = parse_length(len_text, &cal.unit).unwrap_or(f64::NAN);
    if !(mm > 0.0) {
        return Err("sketch.err.length");
    }
    let len = mm / cal.mm_per_pt;
    let (dx, dy);
    match ang_text.map(crate::js::trim).filter(|a| !a.is_empty()) {
        Some(a) => {
            let a = a.strip_suffix('°').unwrap_or(a);
            let deg = crate::js::str_to_number(a);
            if !deg.is_finite() {
                return Err("sketch.err.angle");
            }
            let r = if rot != 0.0 && !rot.is_nan() { rot } else { 0.0 };
            let ang = ((deg + r) * std::f64::consts::PI) / 180.0;
            dx = crate::js::JsMath::jcos(ang);
            dy = crate::js::JsMath::jsin(ang);
        }
        None => {
            let (vx, vy) = (toward[0] - from[0], toward[1] - from[1]);
            let d = crate::js::hypot(vx, vy);
            if d < 1e-9 {
                return Err("sketch.err.direction");
            }
            dx = vx / d;
            dy = vy / d;
        }
    }
    Ok([from[0] + dx * len, from[1] + dy * len])
}

/// One legend row.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub label: String,
    pub color: String,
    pub kind: String,
    pub n: f64,
}

/// `legendRows(markups, {sheet, scope, name})` (most first, ≤ 40).
pub fn legend_rows(markups: &[&Markup], sheet: Option<&str>, scope: &str, name: &dyn Fn(&Markup) -> String) -> Vec<Row> {
    let mut by: Vec<(String, Row)> = Vec::new();
    for m in markups {
        if matches!(m.kind.as_str(), "legend" | "foreign" | "space") || m.locked {
            continue;
        }
        if scope != "set" && m.sheet.as_deref() != sheet {
            continue;
        }
        let nm = name(m);
        let base = if m.subject.is_empty() { nm.clone() } else { m.subject.clone() };
        let t = crate::js::trim(&base);
        let label = if t.is_empty() { nm } else { t.to_string() };
        let color = if m.style.color.is_empty() { "#000000".to_string() } else { m.style.color.clone() };
        let k = format!("{label}\u{0}{color}");
        let n = if m.kind == "count" { m.pts.as_ref().map_or(0.0, |p| p.len() as f64) } else { 1.0 };
        match by.iter_mut().find(|(kk, _)| *kk == k) {
            Some((_, r)) => r.n += n,
            None => by.push((k, Row { label, color, kind: m.kind.clone(), n })),
        }
    }
    let mut rows: Vec<Row> = by.into_iter().map(|(_, r)| r).collect();
    rows.sort_by(|a, b| {
        let d = b.n - a.n;
        let d = if d != 0.0 && !d.is_nan() { d } else { locale_compare(&a.label, &b.label) };
        d.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
    });
    rows.truncate(40);
    rows
}

/// `rowsKey(rows)`.
pub fn rows_key(rows: &[Row]) -> String {
    rows.iter().map(|r| format!("{}:{}:{}", r.label, r.color, crate::js::num_to_string(r.n))).collect::<Vec<_>>().join("|")
}
