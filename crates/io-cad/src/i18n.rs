//! i18n.js `t(key, vars)` for this crate: a catalog (key → string or CLDR plural forms), English by default
//! ([`crate::en::EN`], the 0.5 lang/en.json strings for this crate's keys).

use std::sync::OnceLock;

use cc_cad::js;
use serde_json::{Map, Value};

/// A catalog: the strings of one language (`{ key: "text" | { one, other, "=n" … } }`).
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub strings: Map<String, Value>,
    /// the locale for plural rules ("en" picks "one" for exactly 1)
    pub locale: String,
}

impl Catalog {
    /// From a catalog's JSON (`lang/<locale>.json`).
    pub fn new(strings: Map<String, Value>, locale: &str) -> Self {
        Catalog { strings, locale: locale.into() }
    }
    /// The built-in English strings.
    pub fn en() -> &'static Catalog {
        static EN: OnceLock<Catalog> = OnceLock::new();
        EN.get_or_init(|| {
            let strings = serde_json::from_str::<Map<String, Value>>(crate::en::EN).unwrap_or_default();
            Catalog { strings, locale: "en".into() }
        })
    }

    /// The strings of the language now ([`cc_lang::locale`]): English, or English with CadCraft 0.5's es-MX catalog
    /// over it (the page's catalog in 0.5).
    pub fn current() -> &'static Catalog {
        static ES: OnceLock<Catalog> = OnceLock::new();
        if !cc_lang::is_es() {
            return Catalog::en();
        }
        ES.get_or_init(|| {
            let mut strings = Catalog::en().strings.clone();
            let es =
                serde_json::from_str::<Map<String, Value>>(include_str!("../../../assets/lang/cadcraft/es-MX.json")).unwrap_or_default();
            for (k, v) in es {
                if strings.contains_key(&k) {
                    strings.insert(k, v);
                }
            }
            Catalog { strings, locale: "es-MX".into() }
        })
    }

    /// `t(key, vars)`: the string filled, a plural form chosen by `n`; the key itself when the catalog lacks it.
    pub fn t(&self, key: &str, vars: &[(&str, String)]) -> String {
        let Some(v) = self.strings.get(key) else { return key.to_string() };
        let text = match v {
            Value::String(s) => s.clone(),
            Value::Object(forms) => {
                let n = vars.iter().find(|(k, _)| *k == "n").map(|(_, v)| v.as_str());
                self.plural(forms, n).unwrap_or_else(|| key.to_string())
            }
            other => js::to_js_string(other),
        };
        cc_cad::i18n::fill(&text, vars)
    }

    fn plural(&self, forms: &Map<String, Value>, n: Option<&str>) -> Option<String> {
        if let Some(n) = n
            && let Some(Value::String(s)) = forms.get(&format!("={n}"))
        {
            return Some(s.clone());
        }
        let cat = match n {
            None => "other",
            Some(n) => {
                let x = js::str_to_number(n);
                // CLDR en and es: "one" for exactly 1
                let one_rule = self.locale.starts_with("en") || self.locale.starts_with("es");
                if x.is_finite() && one_rule && x == 1.0 { "one" } else { "other" }
            }
        };
        match forms.get(cat) {
            Some(Value::String(s)) => Some(s.clone()),
            _ => forms.get("other").map(|o| if let Value::String(s) = o { s.clone() } else { js::to_js_string(o) }),
        }
    }
}

/// `{ n }` as a fill.
pub fn n(v: f64) -> [(&'static str, String); 1] {
    [("n", js::num_to_string(v))]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plurals() {
        let c = Catalog::en();
        assert_eq!(c.t("dwg.read.splines", &n(1.0)), "1 spline was converted to polylines.");
        assert_eq!(c.t("dwg.read.splines", &n(3.0)), "3 splines were converted to polylines.");
        assert_eq!(c.t("no.such.key", &[]), "no.such.key");
        assert_eq!(c.t("dxf.solid_line", &[]), "Solid line");
    }
}
