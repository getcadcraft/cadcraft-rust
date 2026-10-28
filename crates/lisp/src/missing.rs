//! AutoLISP / Visual LISP functions CadCraft's LISP does not have yet (missing.js). When a routine calls one, "no
//! function definition: NAME" (the language's own error, which routines test for) is followed by one line saying the
//! function exists elsewhere but is not built here yet.

const NAMES: &[&str] = &[
    "GRREAD",
    "DICTADD",
    "DICTNEXT",
    "DICTREMOVE",
    "DICTRENAME",
    "DICTSEARCH",
    "NAMEDOBJDICT",
    "XDROOM",
    "XDSIZE",
    "SNVALID",
    "XLOAD",
    "XUNLOAD",
    "ARX",
    "ARXLOAD",
    "ARXUNLOAD",
    "AUTOARXLOAD",
    "AUTOLOAD",
    "AUTOXLOAD",
    "VLARTS-INIT",
    "ACAD_HELPDLG",
    "INITDIA",
    "TABLET",
    "SETCFG",
    "GETCFG",
    "VLAX-ADD-CMD",
    "VLAX-REMOVE-CMD",
    "VLAX-GET-OR-CREATE-OBJECT",
    "VLAX-CREATE-OBJECT",
    "VLAX-GET-OBJECT",
    "VLAX-IMPORT-TYPE-LIBRARY",
    "VLAX-LDATA-PUT",
    "VLAX-LDATA-GET",
    "VLAX-LDATA-DELETE",
    "VLAX-LDATA-LIST",
    "VLAX-LDATA-TEST",
    "VLAX-DUMP-OBJECT",
    "VLAX-TMATRIX",
    "VLAX-MAP-COLLECTION",
    "VLAX-PRODUCT-KEY",
    "VLAX-MACHINE-PRODUCT-KEY",
    "VLAX-USER-PRODUCT-KEY",
    "VL-VBALOAD",
    "VL-VBARUN",
    "VL-VLX-LOADED-P",
    "VL-LIST-LOADED-VLX",
    "VL-UNLOAD-VLX",
    "VL-LIST-EXPORTED-FUNCTIONS",
    "VL-ANNOTATIVE-GET",
    "VL-ANNOTATIVE-SET",
];
const PREFIXES: &[&str] = &["VLR-", "VLAX-CURVE-", "ACET-", "DOS_", "VLAX-LDATA-"];

/// Is this a function of the language that CadCraft has not built?
pub fn known_missing(name: &str) -> bool {
    let n = name.to_uppercase();
    NAMES.contains(&n.as_str()) || PREFIXES.iter().any(|p| n.starts_with(p))
}

/// The English note (the engine replaces it with the catalog's 'lisp.missing.known').
pub fn english_note(name: &str) -> String {
    format!("CadCraft's LISP does not have {name} yet (Help: Run LISP routines lists what works).")
}
