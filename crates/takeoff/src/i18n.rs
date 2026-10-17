//! TakeoffCraft's words: `t(key, fills)` as `i18n.js` does it, over TakeoffCraft 0.5's own `lang/en.json` (copied
//! unchanged into `assets/lang/takeoffcraft/`). A string is text with `{name}` fills, or a plural object chosen by the
//! fill `n` (`"=n"` exactly, then the English CLDR category, then `other`); `{{` / `}}` are literal braces; a key no
//! catalog has comes back as the key itself.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde_json::Value;

static EN_JSON: &str = include_str!("../../../assets/lang/takeoffcraft/en.json");
static ES_JSON: &str = include_str!("../../../assets/lang/takeoffcraft/es-MX.json");

/// A fill's value: text, or a number (written as JS `String(n)` writes it).
#[derive(Debug, Clone)]
pub enum F {
    S(String),
    N(f64),
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
impl From<f64> for F {
    fn from(n: f64) -> Self {
        F::N(n)
    }
}
impl From<usize> for F {
    fn from(n: usize) -> Self {
        #[allow(clippy::cast_precision_loss)]
        F::N(n as f64)
    }
}
impl F {
    fn text(&self) -> String {
        match self {
            F::S(s) => s.clone(),
            F::N(n) => crate::jv::num_str(*n),
        }
    }
}

fn catalog(json: &'static str) -> HashMap<String, Value> {
    match serde_json::from_str::<Value>(json) {
        Ok(Value::Object(o)) => o.into_iter().collect(),
        _ => HashMap::new(),
    }
}
fn en() -> &'static HashMap<String, Value> {
    static EN: OnceLock<HashMap<String, Value>> = OnceLock::new();
    EN.get_or_init(|| catalog(EN_JSON))
}
/// The es-MX catalog (draft), for a caller that wants it; the engine itself speaks English like the Node test.
pub fn es_mx() -> &'static HashMap<String, Value> {
    static ES: OnceLock<HashMap<String, Value>> = OnceLock::new();
    ES.get_or_init(|| catalog(ES_JSON))
}

/// Is there a string for this key?
pub fn has(key: &str) -> bool {
    en().contains_key(key)
}

/// English CLDR plural category: "one" for exactly 1, else "other".
fn category(n: f64) -> &'static str {
    if n == 1.0 { "one" } else { "other" }
}

fn choose(forms: &serde_json::Map<String, Value>, n: Option<&F>) -> Option<String> {
    if let Some(nv) = n {
        let k = format!("={}", nv.text());
        if let Some(Value::String(s)) = forms.get(&k) {
            return Some(s.clone());
        }
    }
    let cat = match n {
        None => "other",
        Some(F::N(x)) => {
            if x.is_finite() {
                category(*x)
            } else {
                "other"
            }
        }
        Some(F::S(s)) => {
            let x = crate::jv::str_to_number(s);
            if x.is_finite() { category(x) } else { "other" }
        }
    };
    match forms.get(cat) {
        Some(Value::String(s)) => Some(s.clone()),
        _ => match forms.get("other") {
            Some(Value::String(s)) => Some(s.clone()),
            _ => None,
        },
    }
}

/// `fill(text, vars)`.
pub fn fill(text: &str, vars: &[(&str, F)]) -> String {
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
        if b.get(i) == Some(&b'{') {
            // {name}: [A-Za-z_][A-Za-z0-9_]*
            let name_len = rest
                .bytes()
                .skip(1)
                .enumerate()
                .take_while(|(k, c)| c.is_ascii_alphabetic() || *c == b'_' || (*k > 0 && c.is_ascii_digit()))
                .count();
            if name_len > 0 && rest.as_bytes().get(name_len.saturating_add(1)) == Some(&b'}') {
                let name = rest.get(1..name_len.saturating_add(1)).unwrap_or("");
                match vars.iter().find(|(k, _)| *k == name) {
                    Some((_, v)) => out.push_str(&v.text()),
                    None => out.push_str(rest.get(..name_len.saturating_add(2)).unwrap_or("")),
                }
                i = i.saturating_add(name_len.saturating_add(2));
                continue;
            }
        }
        let c = rest.chars().next().unwrap_or('\0');
        out.push(c);
        i = i.saturating_add(c.len_utf8().max(1));
    }
    out
}

/// `t(key)`.
pub fn t(key: &str) -> String {
    tf(key, &[])
}
/// `t(key, vars)`.
pub fn tf(key: &str, vars: &[(&str, F)]) -> String {
    let text = match en().get(key) {
        None => return key.to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Object(forms)) => {
            let n = vars.iter().find(|(k, _)| *k == "n").map(|(_, v)| v);
            match choose(forms, n) {
                Some(s) => s,
                None => key.to_string(),
            }
        }
        Some(_) => key.to_string(),
    };
    fill(&text, vars)
}

/// `t(key, { n })` shorthand.
pub fn tn(key: &str, n: f64) -> String {
    tf(key, &[("n", F::N(n))])
}
