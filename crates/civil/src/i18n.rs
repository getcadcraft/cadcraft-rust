//! i18n.js `t()` over CivilCraft 0.5's own English catalog (`assets/lang/civilcraft/en.json`, unchanged): `{name}`
//! fills, `{{` / `}}`, plural objects chosen by the fill `n` ("=n" exactly, then the English CLDR category, then
//! "other"); a key the catalog lacks comes back as the key itself, as i18n.js does.

use serde_json::Value;
use std::sync::OnceLock;

static EN: &str = include_str!("../../../assets/lang/civilcraft/en.json");

fn catalog() -> &'static serde_json::Map<String, Value> {
    static CAT: OnceLock<serde_json::Map<String, Value>> = OnceLock::new();
    CAT.get_or_init(|| match serde_json::from_str::<Value>(EN) {
        Ok(Value::Object(m)) => m,
        _ => serde_json::Map::new(),
    })
}

/// A fill value: a JS number or a string (`String(v)` is what lands in the text).
#[derive(Clone, Debug)]
pub enum F {
    N(f64),
    S(String),
}
impl F {
    fn text(&self) -> String {
        match self {
            F::N(x) => crate::js::num_to_string(*x),
            F::S(s) => s.clone(),
        }
    }
}
impl From<f64> for F {
    fn from(x: f64) -> Self {
        F::N(x)
    }
}
impl From<usize> for F {
    fn from(x: usize) -> Self {
        F::N(x as f64)
    }
}
impl From<i64> for F {
    fn from(x: i64) -> Self {
        F::N(x as f64)
    }
}
impl From<i32> for F {
    fn from(x: i32) -> Self {
        F::N(f64::from(x))
    }
}
impl From<&str> for F {
    fn from(s: &str) -> Self {
        F::S(s.to_string())
    }
}
impl From<String> for F {
    fn from(s: String) -> Self {
        F::S(s)
    }
}
impl From<&String> for F {
    fn from(s: &String) -> Self {
        F::S(s.clone())
    }
}

/// English CLDR plural category of a number (`Intl.PluralRules('en')`): "one" for ±1 written without a fraction.
fn plural_en(n: f64) -> &'static str {
    if n.abs() == 1.0 { "one" } else { "other" }
}

fn choose_plural<'a>(forms: &'a serde_json::Map<String, Value>, n: Option<&F>) -> Option<&'a str> {
    if let Some(n) = n {
        let key = format!("={}", n.text());
        if let Some(Value::String(s)) = forms.get(&key) {
            return Some(s);
        }
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

/// `fill(text, vars)`: "{name}" → the fill; unknown fills stay as written; "{{" → "{", "}}" → "}".
pub fn fill(text: &str, vars: &[(&str, F)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(rest.get(..i).unwrap_or(""));
        let tail = rest.get(i..).unwrap_or("");
        if tail.starts_with("{{") {
            out.push('{');
            rest = tail.get(2..).unwrap_or("");
            continue;
        }
        if tail.starts_with("}}") {
            out.push('}');
            rest = tail.get(2..).unwrap_or("");
            continue;
        }
        if tail.starts_with('{') {
            let name: String = tail.chars().skip(1).take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            let valid = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
            let n1 = name.len().saturating_add(1);
            let n2 = name.len().saturating_add(2);
            let close = tail.get(n1..).is_some_and(|t| t.starts_with('}'));
            if valid && close {
                match vars.iter().find(|(k, _)| *k == name) {
                    Some((_, v)) => out.push_str(&v.text()),
                    None => out.push_str(tail.get(..n2).unwrap_or("")),
                }
                rest = tail.get(n2..).unwrap_or("");
                continue;
            }
        }
        out.push_str(tail.get(..1).unwrap_or(""));
        rest = tail.get(1..).unwrap_or("");
    }
    out.push_str(rest);
    out
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
        Some(_) => key.to_string(),
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
        assert_eq!(t("no.such.key", &[]), "no.such.key");
        assert_eq!(fill("{a} {{b}} {c}", &[("a", F::N(1.5))]), "1.5 {b} {c}");
        assert!(has("tin.err.noz"));
    }
}
