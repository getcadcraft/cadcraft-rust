//! `typein.js` — typed lengths and coordinates while drawing (T3): `12'6"` toward the pointer, `12'6"<30` at an angle,
//! `@6',-4'` relative, `6',4'` / `#6',4'` from the run's first point. Directions are the screen's. Errors are
//! language-file keys (`typein.err.*`).

use crate::geom::P;
use crate::jsmath::{cos, sin};
use crate::units::parse_length;
use cc_m3d::js::hypot2;

/// A length that may be signed or zero (coordinates). → feet | None
pub fn signed_length(text: &str) -> Option<f64> {
    let s0: String = cc_m3d::js::trim(text).chars().map(|c| if matches!(c, '−' | '–' | '—') { '-' } else { c }).collect();
    let mut sign = 1.0;
    let s = if let Some(r) = s0.strip_prefix('-') {
        sign = -1.0;
        cc_m3d::js::trim(r).to_string()
    } else if let Some(r) = s0.strip_prefix('+') {
        cc_m3d::js::trim(r).to_string()
    } else {
        s0.clone()
    };
    if zero_text(&s) && (s.contains('0') || s.contains('.')) {
        return Some(0.0);
    }
    parse_length(&s, "ft").ok().map(|ft| sign * ft)
}

/// `/^0*(\.0*)?\s*('|"|ft|in|m|cm|mm)?$/i`
fn zero_text(s: &str) -> bool {
    let b: Vec<char> = s.chars().collect();
    let mut i = 0usize;
    while b.get(i) == Some(&'0') {
        i = i.saturating_add(1);
    }
    if b.get(i) == Some(&'.') {
        i = i.saturating_add(1);
        while b.get(i) == Some(&'0') {
            i = i.saturating_add(1);
        }
    }
    while b.get(i).is_some_and(|c| crate::jv::is_space(*c)) {
        i = i.saturating_add(1);
    }
    let rest: String = b.get(i..).map(|r| r.iter().collect()).unwrap_or_default();
    let r = rest.to_ascii_lowercase();
    rest.is_empty() || matches!(r.as_str(), "'" | "\"" | "ft" | "in" | "m" | "cm" | "mm") && rest.is_ascii()
}

/// The user-space unit vector that points along a screen direction (sx right, sy UP), or None.
pub fn screen_dir(linear: &[f64; 4], sx: f64, sy: f64) -> Option<P> {
    let [a, b, c, d] = *linear;
    let det = a * d - b * c;
    if det == 0.0 || det.is_nan() {
        return None;
    }
    let px = sx;
    let py = -sy;
    let ux = (d * px - c * py) / det;
    let uy = (-b * px + a * py) / det;
    let n = hypot2(ux, uy);
    if n == 0.0 || n.is_nan() { None } else { Some(P::new(ux / n, uy / n)) }
}

/// The drawing context of a typed entry.
#[derive(Debug, Clone)]
pub struct Ctx {
    pub first: Option<P>,
    pub last: Option<P>,
    pub cursor: Option<P>,
    pub ft_per_unit: f64,
    pub linear: [f64; 4],
}

/// `parseEntry(text, ctx)` → (point, kind) | error key.
pub fn parse_entry(text: &str, ctx: &Ctx) -> Result<(P, &'static str), &'static str> {
    let s = cc_m3d::js::trim(text);
    if s.is_empty() {
        return Err("typein.err.read");
    }
    if !(ctx.ft_per_unit > 0.0) {
        return Err("typein.err.noscale");
    }
    let k = 1.0 / ctx.ft_per_unit;
    let along = |from: &P, dir: &P, ft: f64| P::new(from.x + dir.x * ft * k, from.y + dir.y * ft * k);
    if s.starts_with('@') || s.contains(',') {
        let rel = s.starts_with('@');
        let body = s.strip_prefix('@').or_else(|| s.strip_prefix('#')).unwrap_or(s);
        let parts: Vec<&str> = body.split(',').collect();
        let [p0, p1] = parts.as_slice() else { return Err("typein.err.read") };
        let (Some(dx), Some(dy)) = (signed_length(p0), signed_length(p1)) else { return Err("typein.err.read") };
        let from = if rel { ctx.last } else { ctx.first };
        let Some(from) = from else { return Err("typein.err.first") };
        if dx == 0.0 && dy == 0.0 {
            return Err("typein.err.zero");
        }
        let (Some(ex), Some(ey)) = (screen_dir(&ctx.linear, 1.0, 0.0), screen_dir(&ctx.linear, 0.0, 1.0)) else {
            return Err("typein.err.read");
        };
        return Ok((P::new(from.x + (ex.x * dx + ey.x * dy) * k, from.y + (ex.y * dx + ey.y * dy) * k), if rel { "rel" } else { "abs" }));
    }
    let Some(last) = ctx.last else { return Err("typein.err.first") };
    if let Some(lt) = s.find('<') {
        let r = parse_length(s.get(..lt).unwrap_or(""), "ft");
        let after = cc_m3d::js::trim(s.get(lt.saturating_add(1)..).unwrap_or(""));
        let deg = crate::jv::str_to_number(after.strip_suffix('°').unwrap_or(after));
        let Ok(ft) = r else { return Err("typein.err.read") };
        if after.is_empty() || !deg.is_finite() {
            return Err("typein.err.read");
        }
        let a = deg * std::f64::consts::PI / 180.0;
        let Some(dir) = screen_dir(&ctx.linear, cos(a), sin(a)) else { return Err("typein.err.read") };
        return Ok((along(&last, &dir, ft), "polar"));
    }
    let Ok(ft) = parse_length(s, "ft") else { return Err("typein.err.read") };
    let (dx, dy) = match ctx.cursor {
        Some(c) => (c.x - last.x, c.y - last.y),
        None => (0.0, 0.0),
    };
    let n = hypot2(dx, dy);
    if !(n > 0.0) {
        return Err("typein.err.dir");
    }
    Ok((along(&last, &P::new(dx / n, dy / n), ft), "dist"))
}
