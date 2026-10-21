//! i18n.js `t()` / `fill()` over a catalog lookup: the ported modules take the lookup (the 0.5 `lang/*.json`
//! catalogs, loaded by the caller) instead of a global, and fill `{name}` fills exactly as the JS does
//! (`{{` → `{`, `}}` → `}`, an unknown or null fill stays as written).

/// A catalog lookup: the string of a key, if the catalog has it.
pub type Lookup<'a> = &'a dyn Fn(&str) -> Option<String>;

/// `fill(text, vars)`.
pub fn fill(text: &str, vars: &[(&str, String)]) -> String {
    let mut out = String::new();
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
                    Some((_, v)) => out.push_str(v),
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

/// `t(key, vars)`: the catalog's string filled, or the key itself when the catalog lacks it.
pub fn t(lookup: Lookup, key: &str, vars: &[(&str, String)]) -> String {
    match lookup(key) {
        Some(s) => fill(&s, vars),
        None => key.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fills() {
        assert_eq!(fill("{a} and {b} {{x}} {c}", &[("a", "1".into()), ("b", "2".into())]), "1 and 2 {x} {c}");
        assert_eq!(fill("{sqin} sq in ({sqft} sq ft)", &[("sqin", "9".into()), ("sqft", "0.06".into())]), "9 sq in (0.06 sq ft)");
    }
}

/// A language catalog `{ key: text | { one, other, "=0", … } }` (i18n.js `t()` with plural forms chosen by `n`, as
/// `choosePlural` does for en / es: "one" for 1, else "other").
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    strings: std::sync::Arc<serde_json::Map<String, serde_json::Value>>,
}

impl Catalog {
    pub fn new(strings: &serde_json::Value) -> Self {
        Catalog { strings: std::sync::Arc::new(strings.as_object().cloned().unwrap_or_default()) }
    }
    pub fn has(&self, key: &str) -> bool {
        self.strings.contains_key(key)
    }
    /// `t(key, vars)`: a missing key prints as itself.
    pub fn t(&self, key: &str, vars: &[(&str, String)]) -> String {
        use serde_json::Value;
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
                let cat = match n.map(crate::js::str_to_number) {
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

static CATALOG: std::sync::RwLock<Option<Catalog>> = std::sync::RwLock::new(None);

/// The catalog the drawing's own messages use (constraint notes from `Doc::change`, the notes `Doc::load` makes):
/// i18n.js keeps one for the page; the engine sets it from the catalog it was given.
pub fn set_catalog(c: Catalog) {
    let mut g = CATALOG.write().unwrap_or_else(std::sync::PoisonError::into_inner);
    *g = Some(c);
}
pub fn catalog() -> Catalog {
    CATALOG.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone().unwrap_or_default()
}
