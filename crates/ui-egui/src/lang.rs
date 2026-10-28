//! The apps' words in the language now ([`cc_lang::locale`]: English, or the es-MX draft over English, as 0.5 builds a
//! page's catalog). Each app reads its own 0.5 catalog (`assets/lang/<app>/*.json`, unchanged); the shell words
//! (Settings, Help, Recent files) come from 0.5's shell catalog (`assets/lang/shell`), the app names and roles from
//! the site catalog (`assets/lang/catalog`), the start screen's from Studio's hub catalog (`assets/lang/studio`).
//!
//! `t(cat, key, fills)` fills `{name}` and picks a plural form by the fill `n` as i18n.js does; a key no file has
//! comes back as the key itself.

use std::sync::OnceLock;

use cc_lang::{Catalog, Locale};
use serde_json::Value;

pub static CADCRAFT: Catalog =
    Catalog::new(include_str!("../../../assets/lang/cadcraft/en.json"), include_str!("../../../assets/lang/cadcraft/es-MX.json"));
pub static MODELCRAFT: Catalog =
    Catalog::new(include_str!("../../../assets/lang/modelcraft/en.json"), include_str!("../../../assets/lang/modelcraft/es-MX.json"));
pub static BIMCRAFT: &Catalog = &cc_bim::draw::tr::CATALOG;
pub static MARKUPCRAFT: Catalog =
    Catalog::new(include_str!("../../../assets/lang/markupcraft/en.json"), include_str!("../../../assets/lang/markupcraft/es-MX.json"));
pub static SHELL: Catalog =
    Catalog::new(include_str!("../../../assets/lang/shell/en.json"), include_str!("../../../assets/lang/shell/es-MX.json"));
pub static SITE: Catalog =
    Catalog::new(include_str!("../../../assets/lang/catalog/en.json"), include_str!("../../../assets/lang/catalog/es-MX.json"));
pub static STUDIO: Catalog =
    Catalog::new(include_str!("../../../assets/lang/studio/en.json"), include_str!("../../../assets/lang/studio/es-MX.json"));

/// Every app catalog by the 0.5 app slug (the key checks walk these).
pub fn by_app(slug: &str) -> Option<&'static Catalog> {
    Some(match slug {
        "cadcraft" => &CADCRAFT,
        "modelcraft" => &MODELCRAFT,
        "bimcraft" => BIMCRAFT,
        "markupcraft" => &MARKUPCRAFT,
        "takeoffcraft" => &cc_takeoff::i18n::CATALOG,
        "civilcraft" => &cc_civil::i18n::CATALOG,
        "detailcraft" => &cc_detail::i18n::CATALOG,
        "partcraft" => &cc_part::i18n::CATALOG,
        "shell" => &SHELL,
        "catalog" => &SITE,
        "studio" => &STUDIO,
        _ => return None,
    })
}

/// i18n.js `t(key, vars)` on a catalog in the language now.
pub fn t(cat: &Catalog, key: &str, vars: &[(&str, String)]) -> String {
    let n = vars.iter().find(|(k, _)| *k == "n").map(|(_, v)| v.as_str());
    let text = match cat.get(key) {
        Some(Value::String(s)) => s.as_str(),
        Some(Value::Object(forms)) => cc_lang::plural(forms, n).unwrap_or(key),
        _ => return key.to_string(),
    };
    cc_cad::i18n::fill(text, vars)
}

/// `t(key)` with no fills.
pub fn t0(cat: &Catalog, key: &str) -> String {
    t(cat, key, &[])
}

/// A shell word (Settings, Help, Recent files …).
pub fn shell(key: &str, vars: &[(&str, String)]) -> String {
    t(&SHELL, key, vars)
}

/// CadCraft's catalog as the engine takes it (`cc_engine::Lang`), in the language now (built once per language).
pub fn cad_lang() -> cc_engine::Lang {
    static EN: OnceLock<cc_engine::Lang> = OnceLock::new();
    static ES: OnceLock<cc_engine::Lang> = OnceLock::new();
    let l = cc_lang::locale();
    let cell = if l == Locale::EsMx { &ES } else { &EN };
    cell.get_or_init(|| cc_engine::Lang::new(&CADCRAFT.merged(l))).clone()
}

/// The 0.5 app whose catalog a UI source file speaks (the key checks).
pub fn app_of_file(file: &str) -> &'static str {
    let f = file.rsplit('/').next().unwrap_or(file);
    let pre = |p: &str| f.starts_with(p);
    if pre("civil") {
        "civilcraft"
    } else if pre("detail") {
        "detailcraft"
    } else if pre("part") {
        "partcraft"
    } else if pre("takeoff") || pre("autocount") || pre("tkbench") {
        "takeoffcraft"
    } else if pre("markup") || pre("pdf") {
        "markupcraft"
    } else if pre("mc") || pre("model") || pre("picmesh") {
        "modelcraft"
    } else if pre("bim") || pre("ifc") {
        "bimcraft"
    } else if pre("hub") {
        "studio"
    } else {
        "cadcraft"
    }
}

impl crate::UiApp {
    /// Speak `l` in every app from now on (the start screen's switch, Settings, `--lang`). The words on screen change at
    /// once; a drawing already open for editing keeps the command line's language until it is opened again (0.5
    /// reloads the page to change language).
    pub fn set_locale(&mut self, l: Locale) {
        cc_lang::set(l);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_follow_the_language() {
        cc_lang::with(Locale::En, || {
            assert_eq!(shell("shell.settings.lang", &[]), "Language");
            cc_lang::set(Locale::EsMx);
            assert_ne!(shell("shell.settings.lang", &[]), "Language");
            assert_eq!(t0(&SHELL, "shell.no.such.key"), "shell.no.such.key");
            let es = CADCRAFT.es().get("ui.new_sheet_layout_title").and_then(Value::as_str).unwrap_or("");
            assert!(!es.is_empty());
            assert_eq!(cad_lang().t("ui.new_sheet_layout_title", &[]), es);
            cc_lang::set(Locale::En);
            assert_eq!(cad_lang().t("ui.new_sheet_layout_title", &[]), CADCRAFT.str("ui.new_sheet_layout_title").unwrap_or(""));
        });
    }
}
