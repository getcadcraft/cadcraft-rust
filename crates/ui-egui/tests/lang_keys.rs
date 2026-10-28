//! AGENTS.md § 7: a key used in code but missing from English fails; the keys each locale lacks are listed.
//!
//! Every `.rs` file under `crates/*/src` is scanned for catalog calls with a literal key (`t("…"`, `t0`, `tr`, `tf`,
//! `tn`, `tv`, `mt`, `bt`, `lang_text`, `shell`, `.s(` …, optionally after one first argument such as a locale or a
//! catalog). The key must be in the English file of the app the code speaks for (by crate; in the UI crate by the
//! function, else by the file's app), or at least in another app's English file (a view that borrows a word).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cc_ui_egui::lang;

const FNS: &[&str] = &["t", "t0", "tr", "tf", "tn", "tv", "mt", "bt", "lang_text", "s", "shell", "err", "terr", "w"];

fn crate_app(krate: &str) -> Option<&'static str> {
    Some(match krate {
        "civil" | "io-civil" => "civilcraft",
        "detail" | "io-detail" => "detailcraft",
        "part" => "partcraft",
        "markup" | "io-markup" => "markupcraft",
        "m3d" | "m3d-tools" | "io-m3d" | "m3d-files" => "modelcraft",
        "bim" | "io-ifc" => "bimcraft",
        "takeoff" | "io-takeoff" => "takeoffcraft",
        "cad" | "engine" | "io-cad" | "gcons" | "cad-geom" | "mcp" => "cadcraft",
        _ => return None,
    })
}

fn is_key(s: &str) -> bool {
    s.len() <= 120
        && s.contains('.')
        && s.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
        && !s.contains("..")
        && !s.ends_with('.')
}

/// (function, key) for each catalog call with a literal key in `src`.
fn calls(src: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let b = src.as_bytes();
    let mut i = 0usize;
    while let Some(off) = src.get(i..).and_then(|r| r.find('(')) {
        let open = i + off;
        // the identifier before '('
        let mut s = open;
        while s > 0 && b.get(s - 1).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_') {
            s -= 1;
        }
        let name = src.get(s..open).unwrap_or("");
        let before_ok = s == 0 || !b.get(s - 1).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_');
        i = open + 1;
        if !before_ok || !FNS.contains(&name) {
            continue;
        }
        let rest = src.get(open + 1..).unwrap_or("").trim_start();
        // an optional first argument (a locale, a catalog): up to a comma, without quotes or parentheses
        let rest = match rest.find(',') {
            Some(c) if !rest.starts_with('"') && rest.get(..c).is_some_and(|a| !a.contains(['"', '(', ')', ';', '{'])) => {
                rest.get(c + 1..).unwrap_or("").trim_start()
            }
            _ => rest,
        };
        let Some(lit) = rest.strip_prefix('"') else { continue };
        let Some(end) = lit.find('"') else { continue };
        let key = lit.get(..end).unwrap_or("");
        if is_key(key) {
            out.push((name.to_string(), key.to_string()));
        }
    }
    out
}

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn keys_used_in_code_are_in_english() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let apps =
        ["cadcraft", "modelcraft", "bimcraft", "markupcraft", "takeoffcraft", "civilcraft", "detailcraft", "partcraft", "shell", "studio"];
    let in_en = |app: &str, key: &str| lang::by_app(app).is_some_and(|c| c.en().contains_key(key));
    let mut missing: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut checked = 0usize;
    let Ok(rd) = std::fs::read_dir(&crates) else { return };
    for c in rd.flatten() {
        let krate = c.file_name().to_string_lossy().into_owned();
        let mut files = Vec::new();
        rs_files(&c.path().join("src"), &mut files);
        for f in files {
            let Ok(src) = std::fs::read_to_string(&f) else { continue };
            let fname = f.display().to_string();
            for (func, key) in calls(&src) {
                let app = match func.as_str() {
                    "mt" => Some("modelcraft"),
                    "bt" => Some("bimcraft"),
                    "shell" => Some("shell"),
                    "lang_text" => Some("cadcraft"),
                    _ if krate == "ui-egui" => Some(lang::app_of_file(&fname)),
                    _ => crate_app(&krate),
                };
                let Some(app) = app else { continue };
                // the lookups' own tests ask for keys no file has on purpose
                if key.contains("no.such") || key.ends_with(".nope") {
                    continue;
                }
                checked += 1;
                if !in_en(app, &key) && !apps.iter().any(|a| in_en(a, &key)) {
                    missing.entry(format!("{app} ({fname})")).or_default().push(key);
                }
            }
        }
    }
    assert!(checked > 1000, "the scan found only {checked} catalog calls");
    assert!(missing.is_empty(), "keys used in code but missing from English: {missing:#?}");
}

#[test]
fn keys_missing_per_locale() {
    // listed, not failed: es-MX ships as a draft (English answers for what it lacks)
    for app in [
        "cadcraft",
        "modelcraft",
        "bimcraft",
        "markupcraft",
        "takeoffcraft",
        "civilcraft",
        "detailcraft",
        "partcraft",
        "shell",
        "studio",
        "catalog",
    ] {
        let Some(c) = lang::by_app(app) else { continue };
        let miss = c.missing_in(cc_lang::Locale::EsMx);
        eprintln!(
            "{app}: es-MX lacks {} of {} keys{}",
            miss.len(),
            c.en().len(),
            if miss.is_empty() {
                String::new()
            } else {
                format!(" (first: {})", miss.iter().take(5).copied().collect::<Vec<_>>().join(", "))
            }
        );
    }
}

#[test]
fn the_scanner_finds_calls() {
    let v = calls(r#"let a = t("a.b", &[]); x.t(loc, "c.d"); shell("shell.x"); mt("m.y", &[]); nt("no.no"); t(&format!("x"), "z.z")"#);
    let keys: Vec<&str> = v.iter().map(|(_, k)| k.as_str()).collect();
    assert_eq!(keys, vec!["a.b", "c.d", "shell.x", "m.y"]);
}
