//! `pdfset.js` — the open plan set: a PDF is known by the SHA-256 of its bytes; a sheet is "<hash>:<page index>";
//! a page label that is more than the page's own number is its sheet number.

use serde_json::{Map, Value, json};

use cc_takeoff::jv;

/// A page label that is more than the page's own number is taken as the sheet number (A-101).
pub fn sheet_number(label: &str, index: f64) -> String {
    if label.is_empty() || label == cc_takeoff::jv::num_str(index + 1.0) { String::new() } else { label.to_string() }
}

/// The name a sheet goes by: "A-101", "A-101 — Set B" (several files open), or "Set B, page 3".
pub fn sheet_title(file_name: &str, page: &Value, multi_file: bool) -> String {
    let lower = file_name.to_ascii_lowercase();
    let fname = if lower.ends_with(".pdf") { file_name.get(..file_name.len().saturating_sub(4)).unwrap_or(file_name) } else { file_name };
    let number = page.get("number").filter(|v| jv::truthy(Some(v))).map(|v| jv::to_string(Some(v)));
    match number {
        None => format!("{fname}, page {}", cc_takeoff::jv::num_str(jv::to_number(page.get("index")) + 1.0)),
        Some(nb) => {
            if multi_file {
                format!("{nb} — {fname}")
            } else {
                nb
            }
        }
    }
}

/// Match a project's files to the open PDFs: the hash decides; the name only explains a miss.
pub fn match_files(project: &[Value], open: &[Value]) -> Value {
    let mut matched = Map::new();
    let mut missing = Vec::new();
    let mut changed = Vec::new();
    for pf in project {
        if let Some(f) = open.iter().rev().find(|o| jv::strict_eq(o.get("hash"), pf.get("hash"))) {
            matched.insert(jv::to_string(pf.get("hash")), f.clone());
            continue;
        }
        if let Some(same) = open.iter().find(|o| jv::strict_eq(o.get("name"), pf.get("name"))) {
            changed.push(json!({ "want": pf, "have": same }));
        } else {
            missing.push(pf.clone());
        }
    }
    json!({ "matched": matched, "missing": missing, "changed": changed })
}
