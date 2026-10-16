//! `t(key, fills)` for the tools, as i18n.js does it: the tools' catalog ([`crate::catalog`]) first, then the kernel's
//! (`cc_m3d::i18n`), else the key itself. A plural entry picks its form by the fill `n` (English: "one" when n is
//! exactly 1, else "other"; no `n` → "other").

use crate::catalog::{EN, Entry};

fn find(key: &str) -> Option<&'static Entry> {
    static INDEX: std::sync::OnceLock<std::collections::HashMap<&'static str, &'static Entry>> = std::sync::OnceLock::new();
    INDEX.get_or_init(|| EN.iter().map(|(k, v)| (*k, v)).collect()).get(key).copied()
}

/// Is there a string for this key?
pub fn has(key: &str) -> bool {
    find(key).is_some() || cc_m3d::i18n::lookup(key).is_some()
}

/// The string for `key` with its fills (each fill already written as the JS's `String(value)` would write it).
pub fn t(key: &str, vars: &[(&str, &str)]) -> String {
    match find(key) {
        Some(Entry::S(s)) => cc_m3d::i18n::fill(s, vars),
        Some(Entry::P(one, other)) => {
            let n = vars.iter().find(|(k, _)| *k == "n").map(|(_, v)| *v);
            // Intl.PluralRules('en').select(x): "one" for 1 only (Number("1") = 1; "1.0" is written "1" by String())
            let text = if n.is_some_and(|v| v.trim().parse::<f64>().is_ok_and(|x| x == 1.0)) { one } else { other };
            cc_m3d::i18n::fill(text, vars)
        }
        None => cc_m3d::i18n::t(key, vars),
    }
}

/// `t(key)` with no fills.
pub fn t0(key: &str) -> String {
    t(key, &[])
}

/// A number as `String(x)` writes it (for fills).
pub fn n(x: f64) -> String {
    cc_m3d::js::num_str(x)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn words() {
        assert_eq!(t0("m3d.tip.end"), "Endpoint");
        assert_eq!(t("m3d.tool.paint_faces", &[("n", "1")]), "1 face");
        assert_eq!(t("m3d.tool.paint_faces", &[("n", "3")]), "3 faces");
        assert_eq!(t0("m3d.model.dim_angle_only"), "An angle dimension measures only: rotate the edges to change the angle.");
        assert_eq!(t0("m3d.nope"), "m3d.nope");
    }
}
