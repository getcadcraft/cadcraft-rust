//! The language catalog the engine prints from: i18n.js `t()` / `has()` over a `lang/*.json` catalog given by the
//! caller (strings, or plural forms `{ "=0", one, other }` chosen by `n` as i18n.js `choosePlural` does).

use cc_cad::i18n::fill;
use serde_json::{Map, Value};

/// Shared: a clone (a command holding the catalog across a change) shares the strings.
#[derive(Clone, Debug, Default)]
pub struct Lang {
    strings: std::sync::Arc<Map<String, Value>>,
}

impl Lang {
    /// A catalog `{ key: text | {one, other, …} }` (anything else → an empty catalog: every key prints as itself).
    pub fn new(strings: &Value) -> Self {
        Lang { strings: std::sync::Arc::new(strings.as_object().cloned().unwrap_or_default()) }
    }
    /// `has(key)`
    pub fn has(&self, key: &str) -> bool {
        self.strings.contains_key(key)
    }
    /// `t(key, vars)`: a missing key prints as itself.
    pub fn t(&self, key: &str, vars: &[(&str, String)]) -> String {
        match self.strings.get(key) {
            None => key.to_string(),
            Some(Value::String(s)) => fill(s, vars),
            Some(Value::Object(forms)) => {
                let n = vars.iter().find(|(k, _)| *k == "n").map(|(_, v)| v.as_str());
                if let Some(n) = n
                    && let Some(Value::String(s)) = forms.get(&format!("={n}"))
                {
                    return fill(s, vars);
                }
                // CLDR (en, es): "one" for 1, else "other"
                let cat = match n.map(cc_cad::js::str_to_number) {
                    Some(x) if x.is_finite() && x.abs() == 1.0 => "one",
                    _ => "other",
                };
                let pick = match forms.get(cat) {
                    Some(Value::String(s)) => Some(s),
                    _ => match forms.get("other") {
                        Some(Value::String(s)) => Some(s),
                        _ => None,
                    },
                };
                pick.map_or_else(|| key.to_string(), |s| fill(s, vars))
            }
            Some(_) => key.to_string(),
        }
    }
    /// `t(key)` with no fills.
    pub fn s(&self, key: &str) -> String {
        self.t(key, &[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn plurals() {
        let l = Lang::new(&json!({ "a": "x {n}", "p": { "=0": "none", "one": "{n} thing", "other": "{n} things" } }));
        assert_eq!(l.t("p", &[("n", "0".into())]), "none");
        assert_eq!(l.t("p", &[("n", "1".into())]), "1 thing");
        assert_eq!(l.t("p", &[("n", "3".into())]), "3 things");
        assert_eq!(l.t("a", &[("n", "3".into())]), "x 3");
        assert_eq!(l.t("missing", &[]), "missing");
        assert!(l.has("a") && !l.has("b"));
    }
}
