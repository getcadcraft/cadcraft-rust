//! The words MarkupCraft's modules say: `t(key, fills)` as `i18n.js` does it, over the English strings of the 0.5
//! catalog ([`crate::en::EN`]). A key no catalog has comes back as the key itself (i18n.js returns the key and
//! remembers it as missing). Plural entries choose `=n` exactly, then the English CLDR category (`one` for the number
//! 1, else `other`), then `other`, like `choosePlural`.

use crate::en::EN;

/// One catalog entry: plain text with `{name}` fills, or plural forms.
#[derive(Debug, Clone, Copy)]
pub enum Msg {
    S(&'static str),
    P(&'static [(&'static str, &'static str)]),
}

/// A fill value: text, or a number (printed as `String(n)`; also what chooses a plural form when named `n`).
#[derive(Debug, Clone)]
pub enum Fill {
    S(String),
    N(f64),
}

impl From<&str> for Fill {
    fn from(s: &str) -> Self {
        Fill::S(s.to_string())
    }
}
impl From<String> for Fill {
    fn from(s: String) -> Self {
        Fill::S(s)
    }
}
impl From<&String> for Fill {
    fn from(s: &String) -> Self {
        Fill::S(s.clone())
    }
}
impl From<f64> for Fill {
    fn from(n: f64) -> Self {
        Fill::N(n)
    }
}
impl From<usize> for Fill {
    fn from(n: usize) -> Self {
        // counts here are list lengths far below 2^53
        #[allow(clippy::cast_precision_loss)]
        Fill::N(n as f64)
    }
}

impl Fill {
    fn text(&self) -> String {
        match self {
            Fill::S(s) => s.clone(),
            Fill::N(n) => cc_cad::js::num_to_string(*n),
        }
    }
}

fn lookup(key: &str) -> Option<Msg> {
    EN.binary_search_by(|(k, _)| (*k).cmp(key)).ok().and_then(|i| EN.get(i)).map(|(_, m)| *m)
}

/// Is there a string for this key?
pub fn has(key: &str) -> bool {
    lookup(key).is_some()
}

/// `fill(text, vars)`: `{name}` → the value (unknown fills stay as written), `{{` / `}}` → literal braces.
pub fn fill(text: &str, vars: &[(&str, Fill)]) -> String {
    let mut out = String::with_capacity(text.len());
    let b = text.as_bytes();
    let mut i = 0usize;
    let mut last = 0usize;
    while i < b.len() {
        let c = b.get(i).copied().unwrap_or(0);
        if c == b'{' && b.get(i.saturating_add(1)) == Some(&b'{') {
            out.push_str(text.get(last..i).unwrap_or(""));
            out.push('{');
            i = i.saturating_add(2);
            last = i;
            continue;
        }
        if c == b'}' && b.get(i.saturating_add(1)) == Some(&b'}') {
            out.push_str(text.get(last..i).unwrap_or(""));
            out.push('}');
            i = i.saturating_add(2);
            last = i;
            continue;
        }
        if c == b'{' {
            // {[A-Za-z_][A-Za-z0-9_]*}
            let start = i.saturating_add(1);
            let mut j = start;
            while let Some(&d) = b.get(j) {
                let ok = d == b'_' || d.is_ascii_alphabetic() || (j > start && d.is_ascii_digit());
                if !ok {
                    break;
                }
                j = j.saturating_add(1);
            }
            if j > start && b.get(j) == Some(&b'}') {
                let name = text.get(start..j).unwrap_or("");
                out.push_str(text.get(last..i).unwrap_or(""));
                match vars.iter().find(|(k, _)| *k == name) {
                    Some((_, v)) => out.push_str(&v.text()),
                    None => out.push_str(text.get(i..=j).unwrap_or("")),
                }
                i = j.saturating_add(1);
                last = i;
                continue;
            }
        }
        i = i.saturating_add(1);
    }
    out.push_str(text.get(last..).unwrap_or(""));
    out
}

/// `t(key, vars)`.
pub fn t(key: &str, vars: &[(&str, Fill)]) -> String {
    match lookup(key) {
        None => key.to_string(),
        Some(Msg::S(s)) => fill(s, vars),
        Some(Msg::P(forms)) => {
            let n = vars.iter().find(|(k, _)| *k == "n").map(|(_, v)| v.clone());
            let text = choose(forms, n.as_ref()).unwrap_or(key);
            fill(text, vars)
        }
    }
}

/// `t(key)` with no fills.
pub fn t0(key: &str) -> String {
    t(key, &[])
}

fn choose(forms: &'static [(&'static str, &'static str)], n: Option<&Fill>) -> Option<&'static str> {
    let get = |k: &str| forms.iter().find(|(f, _)| *f == k).map(|(_, s)| *s);
    if let Some(n) = n {
        let exact = format!("={}", n.text());
        if let Some(s) = get(&exact) {
            return Some(s);
        }
    }
    let cat = match n {
        None => "other",
        Some(f) => {
            let x = match f {
                Fill::N(x) => *x,
                Fill::S(s) => cc_cad::js::str_to_number(s),
            };
            // Intl.PluralRules('en'): "one" for exactly 1 (with no visible fraction digits), else "other"
            if x == 1.0 { "one" } else { "other" }
        }
    };
    get(cat).or_else(|| get("other"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fills_and_plurals() {
        assert_eq!(fill("a {x} {{y}} {z}", &[("x", Fill::from("1"))]), "a 1 {y} {z}");
        assert_eq!(t("legend.more", &[("n", Fill::N(1.0))]), "+ 1 more kind");
        assert_eq!(t("legend.more", &[("n", Fill::N(3.0))]), "+ 3 more kinds");
        assert_eq!(t0("no.such.key"), "no.such.key");
    }
}
