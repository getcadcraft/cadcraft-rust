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
