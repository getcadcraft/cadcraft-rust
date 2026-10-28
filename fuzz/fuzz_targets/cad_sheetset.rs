//! Fuzz sheetset.js's port (SHEETSET, M4.34): any text as a .ccsheets file is read into a set within the limits or
//! refused with a sentence, never a panic, hang or OOM; a set read back writes and re-reads (stable after one round), and survives
//! numbering, renumbering, the field values, the sheet list's objects and every edit.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() > 1 << 20 {
        return;
    }
    let text = String::from_utf8_lossy(data);
    let l = cc_engine::Lang::new(&serde_json::Value::Null);
    let Ok((set, _)) = cc_engine::sheetset::read_sheet_set(&text, &l) else { return };
    let out = cc_engine::sheetset::write_sheet_set(&set);
    // a written set reads back; as in the JS a name cut at 255 may end in spaces that the next read trims, so the
    // text is stable from the second round on
    let (again, _) = cc_engine::sheetset::read_sheet_set(&out, &l).expect("a written set reads back");
    let out2 = cc_engine::sheetset::write_sheet_set(&again);
    let (third, _) = cc_engine::sheetset::read_sheet_set(&out2, &l).expect("a written set reads back");
    assert_eq!(cc_engine::sheetset::write_sheet_set(&third), out2);
    let n = cc_engine::sheetset::number_by_rule(&set, None);
    let n = cc_engine::sheetset::renumber(&n, None, "A-", 101.0, 1.0, 3.0);
    let all = cc_engine::sheetset::all_sheets(&n);
    for x in all.iter().take(50) {
        let _ = cc_engine::sheetset::field_values(&n, &x.sheet, &l);
        let _ = cc_engine::sheetset::layout_name_for(&n, &x.sheet);
    }
    let _ = cc_engine::sheetset::sheet_list_entities(&n, false, None, &l);
    if let Some(first) = all.first().and_then(|x| x.sheet.get("id")).and_then(|v| v.as_str()) {
        let m = cc_engine::sheetset::move_sheet(&n, first, None, Some(1.0));
        let m = cc_engine::sheetset::remove_sheet(&m, first);
        let (m, g) = cc_engine::sheetset::add_subset(&m, "G", None, &l);
        let _ = cc_engine::sheetset::remove_subset(&m, &g);
    }
});
