//! Fuzz CadCraft's input styles engine (cc_engine::inputprof, app-input.js): a `.ccinput.json` and the kept state
//! come from files and other windows, so any text must be read as a profile or refused, never panic; a profile read
//! must export to a text that reads back, and after one round trip it stays the same (a name cut at 40 can end in a
//! space that the next read trims, as the JS does); the key matcher must take any keys.
#![no_main]

use std::sync::OnceLock;

use cc_engine::inputprof::{self as ip, JsMap, KeyMatcher, Profile, RegCmd, Registry};
use libfuzzer_sys::fuzz_target;

fn registry() -> &'static Registry {
    static R: OnceLock<Registry> = OnceLock::new();
    R.get_or_init(|| {
        let c = |id: &str, keys: &[&str], fixed: bool| RegCmd {
            id: id.into(),
            def_keys: keys.iter().map(|k| (*k).to_string()).collect(),
            fixed,
            key_only: false,
            optional: false,
        };
        let commands = vec![
            c("file.open", &["Ctrl+O"], false),
            c("file.save", &["Ctrl+S"], false),
            c("draw.line", &[], false),
            c("draw.circle", &[], false),
            c("edit.undo", &["Ctrl+Z"], false),
            c("edit.delete", &["Delete"], true),
            c("aids.osnap", &["F3"], false),
        ];
        let mut keys = JsMap::new();
        for x in commands.iter().filter(|x| !x.def_keys.is_empty() && !x.fixed) {
            keys.set(&x.id, x.def_keys.clone());
        }
        let own = Profile { id: "own".into(), name: "input.style.own".into(), builtin: true, keys: Some(keys.clone()), ..Profile::default() };
        let ribbon = Profile {
            id: "ribbon".into(),
            name: "input.style.ribbon".into(),
            builtin: true,
            layout: "ribbon".into(),
            menus: Some(vec![ip::Menu { label: "input.m.file".into(), items: vec!["file.open".into(), "-".into(), "file.save".into()] }]),
            quick: Some(vec!["edit.undo".into()]),
            tabs: Some(vec![ip::RTab {
                label: "input.m.home".into(),
                panels: vec![ip::RPanel { label: "input.m.draw".into(), items: vec!["draw.line".into(), "draw.circle".into()] }],
            }]),
            keys: Some(keys),
            ..Profile::default()
        };
        Registry { commands, styles: vec![own, ribbon], family: vec![("own".into(), "own".into()), ("ribbon".into(), "ribbon".into())], macros: true }
    })
}

fuzz_target!(|data: &[u8]| {
    let reg = registry();
    let text = String::from_utf8_lossy(data);
    if let Ok((mut p, _)) = ip::import_profile(&text, reg) {
        // what was read exports to a text that reads back as the same profile
        let out = ip::export_profile(&p);
        let (back, _) = ip::import_profile(&out, reg).unwrap_or_else(|e| panic!("an exported profile was refused: {e:?}\n{out}"));
        let out2 = ip::export_profile(&back);
        let (back2, _) = ip::import_profile(&out2, reg).unwrap_or_else(|e| panic!("an exported profile was refused: {e:?}\n{out2}"));
        assert_eq!(back2, back, "read back differently:\n{out2}");
        p.id = "my-1".into();
        let r = ip::resolve(&p, reg);
        let _ = ip::unplaced(&r, reg);
        let mut m = KeyMatcher::new(&r, reg);
        for w in text.split_whitespace().take(64) {
            let _ = m.press(&ip::norm_combo(w).unwrap_or_default());
        }
        let q = ip::move_command(&r, "draw.line", text.get(..3).unwrap_or(""));
        let _ = ip::set_macro(&q, text.get(..8).unwrap_or(""), &text);
    }
    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(data) {
        let st = ip::read_state(&ip::write_state(&ip::read_state(&v, reg)), reg);
        let again = ip::read_state(&ip::write_state(&st), reg);
        assert_eq!(again, st, "the kept state reads back differently");
        let _ = ip::carry_shortcuts(&v);
    }
    let _ = ip::norm_seq(&text);
    let _ = ip::export_name(&text);
});
