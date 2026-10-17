//! i18n.js `t()` over DetailCraft 0.5's own English catalog (`crates/detail/lang/en.json`, the frozen
//! `dev/lang/en.json`, unchanged): `{name}` fills (cc-civil's `fill`, the same rules), plural objects chosen by the
//! fill `n` ("=n" exactly, then the English CLDR category, then "other"); a key the catalog lacks comes back as the
//! key itself, as i18n.js does.

pub use cc_civil::i18n::{F, fill};
use serde_json::Value;
use std::sync::OnceLock;

static EN: &str = include_str!("../lang/en.json");
/// The es-MX draft catalog (shipped as draft; English is the fallback).
pub static ES_MX: &str = include_str!("../lang/es-MX.json");

fn catalog() -> &'static serde_json::Map<String, Value> {
    static CAT: OnceLock<serde_json::Map<String, Value>> = OnceLock::new();
    CAT.get_or_init(|| match serde_json::from_str::<Value>(EN) {
        Ok(Value::Object(m)) => m,
        _ => serde_json::Map::new(),
    })
}

fn text_of(v: &F) -> String {
    match v {
        F::N(x) => crate::js::num_to_string(*x),
        F::S(s) => s.clone(),
    }
}

/// English CLDR plural category (`Intl.PluralRules('en')`): "one" for exactly ±1.
fn plural_en(n: f64) -> &'static str {
    if n.abs() == 1.0 { "one" } else { "other" }
}

fn choose_plural<'a>(forms: &'a serde_json::Map<String, Value>, n: Option<&F>) -> Option<&'a str> {
    if let Some(n) = n
        && let Some(Value::String(s)) = forms.get(&format!("={}", text_of(n)))
    {
        return Some(s);
    }
    let cat = match n {
        None => "other",
        Some(F::N(x)) if x.is_finite() => plural_en(*x),
        Some(F::N(_)) => "other",
        Some(F::S(s)) => {
            let x = crate::js::str_to_number(s);
            if x.is_finite() { plural_en(x) } else { "other" }
        }
    };
    match forms.get(cat) {
        Some(Value::String(s)) => Some(s),
        _ => forms.get("other").and_then(Value::as_str),
    }
}

/// `t(key, vars)`.
pub fn t(key: &str, vars: &[(&str, F)]) -> String {
    match catalog().get(key) {
        None => key.to_string(),
        Some(Value::String(s)) => fill(s, vars),
        Some(Value::Object(forms)) => {
            let n = vars.iter().find(|(k, _)| *k == "n").map(|(_, v)| v);
            match choose_plural(forms, n) {
                Some(s) => fill(s, vars),
                None => key.to_string(),
            }
        }
        // a value that is neither text nor plural forms: `fill(text ?? key)` of its String()
        Some(v) => fill(&crate::js::to_js_string(v), vars),
    }
}

/// `t(key)` without fills.
pub fn t0(key: &str) -> String {
    t(key, &[])
}

/// `has(key)`.
pub fn has(key: &str) -> bool {
    catalog().contains_key(key)
}

/// `tr!("key", n = 3, name = s)` → `t("key", &[("n", F::from(3)), ("name", F::from(s))])`.
#[macro_export]
macro_rules! tr {
    ($key:expr) => { $crate::i18n::t($key, &[]) };
    ($key:expr, $($k:ident = $v:expr),+ $(,)?) => {
        $crate::i18n::t($key, &[$((stringify!($k).trim_start_matches("r#"), $crate::i18n::F::from($v))),+])
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strings() {
        assert_eq!(t0("no.such.key"), "no.such.key");
        assert!(has("model.err.not-json"));
        assert!(!ES_MX.is_empty());
    }
}
