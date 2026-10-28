//! CadCraft L0: the language the apps speak, and the 0.5 language catalogs as pairs (English + Spanish (Mexico)).
//!
//! 0.5 renders each page with one catalog chosen by the server (PHP `Lang`): the locale's file over English, so a key
//! the translation lacks shows in English, and a key no file has prints as the key itself (i18n.js `t()`). The apps
//! here do the same: [`Catalog::get`] looks in the Spanish file first while the locale is es-MX, then in English.
//!
//! The locale is process-wide ([`set`]; the start screen's language switch, Settings, `--lang`), so worker threads
//! (rayon, a rebuild thread) speak it too. Tests pin it per thread with [`with`]: inside, [`set`] changes only that
//! thread's language, so tests running side by side never see each other's language.

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]

use std::cell::Cell;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use serde_json::{Map, Value};

/// A language the apps ship (0.5: en, and es-MX as a draft).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Locale {
    #[default]
    En,
    EsMx,
}

impl Locale {
    /// Every shipped locale, English first.
    pub const ALL: [Locale; 2] = [Locale::En, Locale::EsMx];

    /// The tag 0.5 uses ("en", "es-MX").
    pub fn code(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::EsMx => "es-MX",
        }
    }

    /// The language's own name (the language menus show each in itself).
    pub fn name(self) -> &'static str {
        match self {
            Locale::En => "English",
            Locale::EsMx => "Español (México)",
        }
    }

    /// A tag → its locale ("en", "en-US" → English; "es", "es-MX", "es-419" → Spanish (Mexico)); anything else → None.
    pub fn parse(tag: &str) -> Option<Locale> {
        let t = tag.trim();
        if t.len() > 35 {
            return None;
        }
        let primary = t.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
        match primary.as_str() {
            "en" => Some(Locale::En),
            "es" => Some(Locale::EsMx),
            _ => None,
        }
    }

    fn from_u8(v: u8) -> Locale {
        if v == 1 { Locale::EsMx } else { Locale::En }
    }

    fn to_u8(self) -> u8 {
        match self {
            Locale::En => 0,
            Locale::EsMx => 1,
        }
    }
}

static GLOBAL: AtomicU8 = AtomicU8::new(0);

thread_local! {
    static PINNED: Cell<Option<Locale>> = const { Cell::new(None) };
}

/// The language now (this thread's pinned one inside [`with`], else the process's).
pub fn locale() -> Locale {
    PINNED.with(Cell::get).unwrap_or_else(|| Locale::from_u8(GLOBAL.load(Ordering::Relaxed)))
}

/// Is the language Spanish?
pub fn is_es() -> bool {
    locale() == Locale::EsMx
}

/// Speak `l` from now on (inside [`with`]: on this thread only).
pub fn set(l: Locale) {
    let pinned = PINNED.with(|p| {
        if p.get().is_some() {
            p.set(Some(l));
            true
        } else {
            false
        }
    });
    if !pinned {
        GLOBAL.store(l.to_u8(), Ordering::Relaxed);
    }
}

/// Run `f` with this thread's language pinned to `l` (tests); the previous state comes back afterwards.
pub fn with<R>(l: Locale, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<Locale>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let v = self.0;
            PINNED.with(|p| p.set(v));
        }
    }
    let _restore = Restore(PINNED.with(|p| p.replace(Some(l))));
    f()
}

/// One app's catalog pair (its `lang/en.json` and `lang/es-MX.json`, unchanged), parsed on first use.
#[derive(Debug)]
pub struct Catalog {
    en_json: &'static str,
    es_json: &'static str,
    en: OnceLock<Map<String, Value>>,
    es: OnceLock<Map<String, Value>>,
}

fn parse(json: &str) -> Map<String, Value> {
    match serde_json::from_str::<Value>(json) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

impl Catalog {
    pub const fn new(en_json: &'static str, es_json: &'static str) -> Catalog {
        Catalog { en_json, es_json, en: OnceLock::new(), es: OnceLock::new() }
    }

    /// The English file.
    pub fn en(&self) -> &Map<String, Value> {
        self.en.get_or_init(|| parse(self.en_json))
    }

    /// The Spanish file alone (its missing keys are not filled in).
    pub fn es(&self) -> &Map<String, Value> {
        self.es.get_or_init(|| parse(self.es_json))
    }

    /// A locale's own file.
    pub fn file(&self, l: Locale) -> &Map<String, Value> {
        match l {
            Locale::En => self.en(),
            Locale::EsMx => self.es(),
        }
    }

    /// A key's value in the language now (the Spanish file, then English).
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.get_in(locale(), key)
    }

    /// A key's value in `l` (its file, then English).
    pub fn get_in(&self, l: Locale, key: &str) -> Option<&Value> {
        if l == Locale::EsMx
            && let Some(v) = self.es().get(key)
        {
            return Some(v);
        }
        self.en().get(key)
    }

    /// A key's plain string in the language now (None: missing, or a plural object).
    pub fn str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::as_str)
    }

    /// Does any file have the key?
    pub fn has(&self, key: &str) -> bool {
        self.en().contains_key(key) || self.es().contains_key(key)
    }

    /// The catalog as one object in the language now (English with the locale's strings over it): what 0.5 embeds in
    /// the page, for callers that take a whole catalog (cc-engine `Lang`).
    pub fn merged(&self, l: Locale) -> Value {
        let mut m = self.en().clone();
        if l == Locale::EsMx {
            for (k, v) in self.es() {
                m.insert(k.clone(), v.clone());
            }
        }
        Value::Object(m)
    }

    /// English keys the locale's file lacks (sorted).
    pub fn missing_in(&self, l: Locale) -> Vec<&str> {
        let f = self.file(l);
        let mut v: Vec<&str> = self.en().keys().filter(|k| !f.contains_key(k.as_str())).map(String::as_str).collect();
        v.sort_unstable();
        v
    }
}

/// i18n.js `choosePlural` for en and es: `"=n"` exactly (n as written), then the CLDR category (both languages say
/// "one" for exactly 1, else "other" for the counts the apps print), then "other".
pub fn plural<'a>(forms: &'a Map<String, Value>, n: Option<&str>) -> Option<&'a str> {
    if let Some(n) = n
        && let Some(Value::String(s)) = forms.get(&format!("={n}"))
    {
        return Some(s);
    }
    let one = n.is_some_and(|v| v.trim().parse::<f64>().is_ok_and(|x| x == 1.0));
    let cat = if one { "one" } else { "other" };
    match forms.get(cat) {
        Some(Value::String(s)) => Some(s),
        _ => forms.get("other").and_then(Value::as_str),
    }
}

/// A Spanish-only catalog (crates whose English strings are compiled-in tables): the es-MX file's value of a key while
/// the language is es-MX, else None (the caller's English table answers).
#[derive(Debug)]
pub struct Spanish(Catalog);

impl Spanish {
    pub const fn new(es_json: &'static str) -> Spanish {
        Spanish(Catalog::new("{}", es_json))
    }
    pub fn get(&self, key: &str) -> Option<&Value> {
        if is_es() { self.0.es().get(key) } else { None }
    }
    /// The plain string (a plural object → its form for `n`).
    pub fn text(&self, key: &str, n: Option<&str>) -> Option<&str> {
        match self.get(key)? {
            Value::String(s) => Some(s),
            Value::Object(forms) => plural(forms, n),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static CAT: Catalog = Catalog::new(r#"{"a":"Open","b":"Close","n":{"one":"{n} file","other":"{n} files"}}"#, r#"{"a":"Abrir"}"#);

    #[test]
    fn lookup_falls_back_to_english() {
        with(Locale::En, || {
            assert_eq!(CAT.str("a"), Some("Open"));
            set(Locale::EsMx);
            assert_eq!(locale(), Locale::EsMx);
            assert_eq!(CAT.str("a"), Some("Abrir"));
            assert_eq!(CAT.str("b"), Some("Close"));
            assert_eq!(CAT.str("zz"), None);
            assert!(CAT.get("n").is_some_and(Value::is_object));
            assert_eq!(CAT.missing_in(Locale::EsMx), vec!["b", "n"]);
            assert_eq!(CAT.merged(Locale::EsMx).get("a").and_then(Value::as_str), Some("Abrir"));
        });
    }

    #[test]
    fn pinned_language_stays_on_its_thread() {
        with(Locale::EsMx, || {
            let other = std::thread::spawn(|| with(Locale::En, locale)).join();
            assert!(matches!(other, Ok(Locale::En)));
            assert_eq!(locale(), Locale::EsMx);
        });
    }

    #[test]
    fn tags() {
        assert_eq!(Locale::parse("es-MX"), Some(Locale::EsMx));
        assert_eq!(Locale::parse("es"), Some(Locale::EsMx));
        assert_eq!(Locale::parse("en-US"), Some(Locale::En));
        assert_eq!(Locale::parse("fr"), None);
        assert_eq!(Locale::parse(""), None);
        assert_eq!(Locale::EsMx.code(), "es-MX");
    }
}
