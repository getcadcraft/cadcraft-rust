//! i18n.js `t()` over PartCraft 0.5's own English catalog (`partcraft/dev/lang/en.json`, unchanged): `{name}` fills,
//! `{{` / `}}`, plural objects chosen by the fill `n` ("=n" exactly, then the English CLDR category, then "other"); a key
//! the catalog lacks comes back as the key itself, as i18n.js does. The Spanish (Mexico) catalog ships beside it as a
//! draft (`lang/es-MX.json`), read while the language is es-MX ([`cc_lang::locale`]).

use serde_json::Value;

/// PartCraft 0.5's catalogs (en + the es-MX draft, unchanged).
pub static CATALOG: cc_lang::Catalog = cc_lang::Catalog::new(include_str!("../lang/en.json"), include_str!("../lang/es-MX.json"));

/// A fill value: a JS number or a string (`String(v)` is what lands in the text).
#[derive(Clone, Debug)]
pub enum F {
    N(f64),
    S(String),
}
impl F {
    pub fn text(&self) -> String {
        match self {
            F::N(x) => crate::js::num_str(*x),
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
    #[allow(clippy::cast_precision_loss)]
    fn from(x: usize) -> Self {
        F::N(x as f64)
    }
}
impl From<i64> for F {
    #[allow(clippy::cast_precision_loss)]
    fn from(x: i64) -> Self {
        F::N(x as f64)
    }
}
impl From<i32> for F {
    fn from(x: i32) -> Self {
        F::N(f64::from(x))
    }
}
impl From<u32> for F {
    fn from(x: u32) -> Self {
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

/// Is there a string for this key?
pub fn has(key: &str) -> bool {
    CATALOG.has(key)
}

/// English CLDR plural category (`Intl.PluralRules('en').select(n)`): "one" for exactly 1, else "other".
fn plural_category(n: f64) -> &'static str {
    if n == 1.0 { "one" } else { "other" }
}

/// `fill(text, vars)`: "{name}" → the fill, "{{" / "}}" → braces; unknown fills stay as written.
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
            // {Name} with Name = [A-Za-z_][A-Za-z0-9_]*
            let body = tail.get(1..).unwrap_or("");
            let mut end = 0usize;
            for (k, c) in body.char_indices() {
                let ok = if k == 0 { c.is_ascii_alphabetic() || c == '_' } else { c.is_ascii_alphanumeric() || c == '_' };
                if !ok {
                    break;
                }
                end = k.saturating_add(c.len_utf8());
            }
            if end > 0 && body.get(end..end.saturating_add(1)) == Some("}") {
                let name = body.get(..end).unwrap_or("");
                match vars.iter().find(|(k, _)| *k == name) {
                    Some((_, v)) => out.push_str(&v.text()),
                    None => {
                        out.push('{');
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = body.get(end.saturating_add(1)..).unwrap_or("");
                continue;
            }
        }
        // a lone brace
        out.push_str(tail.get(..1).unwrap_or(""));
        rest = tail.get(1..).unwrap_or("");
    }
    out.push_str(rest);
    out
}

/// `t(key, vars)`.
pub fn t(key: &str, vars: &[(&str, F)]) -> String {
    let Some(v) = CATALOG.get(key) else { return key.to_string() };
    let text: String = match v {
        Value::String(s) => s.clone(),
        Value::Object(forms) => {
            let n = vars.iter().find(|(k, _)| *k == "n").map(|(_, v)| v);
            let pick = |k: &str| forms.get(k).and_then(Value::as_str).map(str::to_string);
            let exact = n.and_then(|n| pick(&format!("={}", n.text())));
            match exact {
                Some(s) => s,
                None => {
                    let cat = match n {
                        Some(F::N(x)) if x.is_finite() => plural_category(*x),
                        Some(F::S(s)) => {
                            let x = cc_cad::js::str_to_number(s);
                            if x.is_finite() { plural_category(x) } else { "other" }
                        }
                        _ => "other",
                    };
                    match pick(cat) {
                        Some(s) => s,
                        None => match forms.get("other") {
                            Some(Value::String(s)) => s.clone(),
                            // `fill(text ?? key)`: a missing "other" form gives the key
                            _ => key.to_string(),
                        },
                    }
                }
            }
        }
        // a value of another type: String(v)
        other => cc_cad::js::to_js_string(other),
    };
    fill(&text, vars)
}

/// `t(key)` with no fills.
pub fn t0(key: &str) -> String {
    t(key, &[])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fills_and_plurals() {
        assert_eq!(fill("a {x} b {{c}} {y}", &[("x", F::N(2.0))]), "a 2 b {c} {y}");
        assert_eq!(t("no.such.key", &[]), "no.such.key");
        assert!(has("model.feature.extrude"));
    }
}
