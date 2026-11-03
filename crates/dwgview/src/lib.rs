//! BimCraft 0.5's DWG view extras without a UI (the page's logic and words; the UI draws them):
//!
//! - [`equipment`]: the equipment schedule from a foodservice drawing's item tags and its CSV (dwg-equipment.js)
//! - [`props`]: what Properties shows for a picked object (dwg-props.js)
//! - [`dynamic`]: dynamic blocks, the states their references show (dwg-dynamic.js)
//! - [`compile3d`]: model space as 3D batches (dwg-compile3d.js)
//! - [`folder`]: linked drawings and pictures found in a project folder (bimcraft.js `findLinked` / `findPicture`,
//!   folder-pick.js's walk rules)
//! - [`page`]: the page's texts for them (bimcraft.js / equip-view.js), in the language now
//!
//! Words: BimCraft's 0.5 catalog (`assets/lang/bimcraft`), through [`tr`].

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

pub mod compile3d;
pub mod dynamic;
pub mod equipment;
pub mod folder;
pub mod page;
pub mod props;

use serde_json::Value;

/// i18n.js `t(key, vars)` on BimCraft's catalog in the language now (a plural form picked by the fill `n`); a key the
/// catalog lacks comes back as itself.
pub fn tr(key: &str, vars: &[(&str, String)]) -> String {
    let cat = &cc_bim::draw::tr::CATALOG;
    let n = vars.iter().find(|(k, _)| *k == "n").map(|(_, v)| v.as_str());
    let text = match cat.get(key) {
        Some(Value::String(s)) => s.as_str(),
        Some(Value::Object(forms)) => cc_lang::plural(forms, n).unwrap_or(key),
        _ => return key.to_string(),
    };
    let refs: Vec<(&str, &str)> = vars.iter().map(|(k, v)| (*k, v.as_str())).collect();
    cc_bim::draw::tr::fill(text, &refs)
}

/// `num(n)` (ui-util.js): en-US grouping whatever the language.
pub fn num(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}
