//! i18n.js `t(key, vars)` on BimCraft's English catalog (`assets/lang/bimcraft/en.json`, the 0.5 file unchanged): the
//! words the building tools write into the model (type names, materials, "Room 1") and the status lines.

use serde_json::Value as J;

/// BimCraft 0.5's catalogs (en + the es-MX draft, unchanged): the language now ([`cc_lang::locale`]), then English.
pub static CATALOG: cc_lang::Catalog = cc_lang::Catalog::new(
    include_str!("../../../../assets/lang/bimcraft/en.json"),
    include_str!("../../../../assets/lang/bimcraft/es-MX.json"),
);

/// Does the catalog have this key?
pub fn has(key: &str) -> bool {
    CATALOG.has(key)
}

/// `fill(text, vars)`: "{name}" → its value, "{{" / "}}" → braces, unknown fills left as written.
pub fn fill(text: &str, vars: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(text.len());
    let b = text.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let rest = text.get(i..).unwrap_or("");
        if rest.starts_with("{{") {
            out.push('{');
            i = i.saturating_add(2);
            continue;
        }
        if rest.starts_with("}}") {
            out.push('}');
            i = i.saturating_add(2);
            continue;
        }
        if rest.starts_with('{')
            && let Some(end) = rest.find('}')
        {
            let name = rest.get(1..end).unwrap_or("");
            let ok = !name.is_empty()
                && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            if ok {
                match vars.iter().find(|(k, _)| *k == name) {
                    Some((_, v)) => out.push_str(v),
                    None => out.push_str(rest.get(..=end).unwrap_or("")),
                }
                i = i.saturating_add(end).saturating_add(1);
                continue;
            }
        }
        let c = rest.chars().next().unwrap_or(' ');
        out.push(c);
        i = i.saturating_add(c.len_utf8());
    }
    out
}

/// English CLDR plural category of n ("one" for exactly 1, else "other").
fn category(n: &str) -> &'static str {
    match n.parse::<f64>() {
        Ok(x) if x == 1.0 && !n.contains('.') => "one",
        _ => "other",
    }
}

/// `t(key, vars)`: the key itself when the catalog has no such string.
pub fn t(key: &str, vars: &[(&str, &str)]) -> String {
    let Some(v) = CATALOG.get(key) else { return key.to_string() };
    let text = match v {
        J::String(s) => s.clone(),
        J::Object(o) => {
            let n = vars.iter().find(|(k, _)| *k == "n").map(|(_, v)| *v);
            let pick = match n {
                Some(n) if o.contains_key(&format!("={n}")) => o.get(&format!("={n}")),
                Some(n) => o.get(category(n)).filter(|x| x.is_string()).or_else(|| o.get("other")),
                None => o.get("other"),
            };
            pick.and_then(J::as_str).unwrap_or(key).to_string()
        }
        _ => key.to_string(),
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
    fn words_from_the_catalog() {
        assert_eq!(t0("model.wt.ext-2x6"), "Exterior 2×6");
        assert_eq!(t("model.room.name", &[("n", "3")]), "Room 3");
        assert_eq!(t0("no.such.key"), "no.such.key");
        assert_eq!(fill("{{a}} {b} {c}", &[("b", "x")]), "{a} x {c}");
    }
}
