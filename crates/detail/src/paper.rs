//! paper.js — the sheet sizes drawings and shop drawings are made on (0.4 D3): ISO A4–A1 and ANSI B–D, landscape, mm.
//! `k` is the sheet's height against A3's 297 mm (at least 0.6).

const IN: f64 = 25.4;

/// A sheet size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Paper {
    pub key: &'static str,
    pub w: f64,
    pub h: f64,
    pub family: &'static str,
    pub inches: Option<&'static str>,
    pub k: f64,
}

/// `PAPER_KEYS`.
pub const PAPER_KEYS: [&str; 7] = ["A4", "A3", "A2", "A1", "ANSI-B", "ANSI-C", "ANSI-D"];
pub const DEFAULT_PAPER: &str = "A3";

fn raw(key: &str) -> Option<(&'static str, f64, f64, &'static str, Option<&'static str>)> {
    Some(match key {
        "A4" => ("A4", 297.0, 210.0, "iso", None),
        "A3" => ("A3", 420.0, 297.0, "iso", None),
        "A2" => ("A2", 594.0, 420.0, "iso", None),
        "A1" => ("A1", 841.0, 594.0, "iso", None),
        "ANSI-B" => ("ANSI-B", 17.0 * IN, 11.0 * IN, "ansi", Some("11 × 17")),
        "ANSI-C" => ("ANSI-C", 22.0 * IN, 17.0 * IN, "ansi", Some("17 × 22")),
        "ANSI-D" => ("ANSI-D", 34.0 * IN, 22.0 * IN, "ansi", Some("22 × 34")),
        _ => return None,
    })
}

/// `paperOf(key)` (unknown → A3).
pub fn paper_of(key: &str) -> Paper {
    let (key, w, h, family, inches) = raw(key).or_else(|| raw(DEFAULT_PAPER)).unwrap_or(("A3", 420.0, 297.0, "iso", None));
    Paper { key, w, h, family, inches, k: crate::js::max(0.6, h / 297.0) }
}

/// `paperLabel(key)`.
pub fn paper_label(key: &str) -> String {
    let p = paper_of(key);
    if p.family == "ansi" {
        format!("{} ({} in)", p.key.replacen('-', " ", 1), p.inches.unwrap_or(""))
    } else {
        format!("{} ({} × {} mm)", p.key, crate::js::num_to_string(p.w), crate::js::num_to_string(p.h))
    }
}

/// `key.replace('-', ' ')` (the first dash).
pub fn key_label(key: &str) -> String {
    key.replacen('-', " ", 1)
}
