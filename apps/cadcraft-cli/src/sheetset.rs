//! Dev: `cadcraft-cli sheetset <script.json>` — sheetset.js / sheetset-io.js / sheetset-archive.js in cc-engine on a
//! script, for `forks/tools/sheetsetdiff.mjs` (which runs the frozen JS on the same script and a copy of the folder).
//!
//! Script: `{ strings, folder?: dir, writable?, created?: ms, steps: [step] }`; a step is `{ op, … }`:
//! `read {text}` · `write {set}` · `new {name}` · `addSheet {set, sheet, subset, at}` · `removeSheet {set, id}` ·
//! `moveSheet {set, id, subset, at}` · `updateSheet {set, id, patch}` · `addSubset {set, name, parent}` ·
//! `renameSubset {set, id, name}` · `removeSubset {set, id}` · `renumber {set, ids, prefix, start, step, digits}` ·
//! `numberByRule {set, numbering}` · `ruleProblem {rule}` · `setProperties {set, p}` · `labels {set}` (allSheets,
//! sheetLabel, layoutNameFor, fieldValues of every sheet) · `fieldTag {tag}` · `cleanPath {p}` · `listEntities {set,
//! inch, height}` · `layouts {path}` · `publish {set, subset, ids, pen, mono, fields, date}` · `update {set, fields,
//! names}` · `addList {set, sheet}` · `archive {set, setFile, setText, today, time, date}` · `resolveRef {ref, from,
//! paths}`. Output per step `{ r }` (or `{ err }`); PDFs come as their inflated Latin-1 form (cmds.rs pdf_for_log).

use std::process::ExitCode;

use cc_engine::Lang;
use cc_engine::sheetset as ss;
use cc_engine::sheetset_io as io;
use serde_json::{Value, json};

fn f(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(Value::as_f64)
}
fn st<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str)
}
fn ids(v: &Value, k: &str) -> Option<Vec<String>> {
    v.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
}
fn set_of(v: &Value) -> Value {
    v.get("set").cloned().unwrap_or(Value::Null)
}

fn step(s: &Value, folder: &mut io::DirFolder, stamp: io::Stamp, l: &Lang) -> Result<Value, String> {
    let set = set_of(s);
    let e = |x: ss::SheetSetError| x.0;
    Ok(match st(s, "op").unwrap_or("") {
        "read" => {
            let (set, notes) = ss::read_sheet_set(st(s, "text").unwrap_or(""), l).map_err(e)?;
            json!({ "set": set, "notes": notes })
        }
        "write" => json!(ss::write_sheet_set(&set)),
        "new" => ss::new_sheet_set(st(s, "name").unwrap_or(""), l),
        "addSheet" => {
            let (n, id) = ss::add_sheet(&set, s.get("sheet").unwrap_or(&Value::Null), st(s, "subset"), f(s, "at"), l).map_err(e)?;
            json!({ "set": n, "id": id })
        }
        "removeSheet" => ss::remove_sheet(&set, st(s, "id").unwrap_or("")),
        "moveSheet" => ss::move_sheet(&set, st(s, "id").unwrap_or(""), st(s, "subset"), f(s, "at")),
        "updateSheet" => ss::update_sheet(&set, st(s, "id").unwrap_or(""), s.get("patch").unwrap_or(&json!({})), l),
        "addSubset" => {
            let (n, id) = ss::add_subset(&set, st(s, "name").unwrap_or(""), st(s, "parent"), l);
            json!({ "set": n, "id": id })
        }
        "renameSubset" => ss::rename_subset(&set, st(s, "id").unwrap_or(""), st(s, "name").unwrap_or("")),
        "removeSubset" => ss::remove_subset(&set, st(s, "id").unwrap_or("")),
        "renumber" => {
            let n = |k: &str, d: f64| s.get(k).map_or(d, |v| cc_cad::js::to_number_opt(Some(v)));
            ss::renumber(&set, ids(s, "ids").as_deref(), st(s, "prefix").unwrap_or(""), n("start", 1.0), n("step", 1.0), n("digits", 0.0))
        }
        "numberByRule" => ss::number_by_rule(&set, s.get("numbering")),
        "ruleProblem" => json!(ss::rule_problem(s.get("rule"))),
        "setProperties" => ss::set_properties(&set, s.get("p").unwrap_or(&Value::Null), l),
        "labels" => json!(
            ss::all_sheets(&set)
                .iter()
                .map(|x| json!({
                    "index": x.index,
                    "path": x.path.iter().map(|g| g.get("id").cloned().unwrap_or(Value::Null)).collect::<Vec<_>>(),
                    "label": ss::sheet_label(&x.sheet),
                    "layoutName": ss::layout_name_for(&set, &x.sheet),
                    "fields": ss::field_values(&set, &x.sheet, l),
                }))
                .collect::<Vec<_>>()
        ),
        "fieldTag" => json!(ss::field_tag(st(s, "tag").unwrap_or(""))),
        "cleanPath" => json!(ss::clean_path(s.get("p"))),
        "listEntities" => json!(ss::sheet_list_entities(&set, s.get("inch").and_then(Value::as_bool).unwrap_or(false), f(s, "height"), l)),
        "layouts" => json!(io::layouts_in(folder, st(s, "path").unwrap_or(""), l)?),
        "publish" => {
            let p = io::publish_set(
                &set,
                folder,
                st(s, "subset"),
                ids(s, "ids").as_deref(),
                f(s, "pen").unwrap_or(0.25),
                s.get("mono").and_then(Value::as_bool).unwrap_or(true),
                s.get("fields").and_then(Value::as_bool).unwrap_or(true),
                f(s, "date").unwrap_or(0.0),
                l,
            );
            json!({ "pdf": p.bytes.as_ref().map(|b| crate::cmds::pdf_for_log(b)), "pages": p.pages, "names": p.names, "missing": p.missing,
                "notes": p.notes })
        }
        "update" => {
            let (n, files, notes) = io::update_drawings(
                &set,
                folder,
                s.get("fields").and_then(Value::as_bool).unwrap_or(true),
                s.get("names").and_then(Value::as_bool).unwrap_or(true),
                stamp,
                l,
            )?;
            json!({ "set": n, "files": files.iter().map(|x| json!({ "path": x.path, "fields": x.fields, "renamed": x.renamed, "list": x.list })).collect::<Vec<_>>(), "notes": notes })
        }
        "addList" => json!({ "ok": true, "path": io::add_sheet_list(&set, folder, s.get("sheet").unwrap_or(&Value::Null), stamp, l)? }),
        "archive" => {
            let read_model = |bytes: &[u8], p: &str| -> Result<cc_io_dwg::value::Obj, String> {
                if p.to_ascii_lowercase().ends_with(".dxf") {
                    cc_io_dwg::dxf_model::try_read_dxf_model(&cc_io_dwg::dxf_model::decode_dxf_bytes(bytes))
                } else {
                    Ok(cc_io_dwg::read_dwg(bytes, &cc_io_dwg::ReadOpts::default()))
                }
            };
            let when = io::DosTime {
                time: u16::try_from(f(s, "time").unwrap_or(0.0) as i64).unwrap_or(0),
                date: u16::try_from(f(s, "date").unwrap_or(0.0) as i64).unwrap_or(0),
            };
            let a = io::archive_set(
                &set,
                folder,
                st(s, "setFile").unwrap_or("set.ccsheets"),
                st(s, "setText"),
                &read_model,
                st(s, "today").unwrap_or(""),
                when,
                l,
            );
            let entries: Vec<Value> = io::unzip(&a.bytes)
                .into_iter()
                .map(|(n, d)| json!({ "name": n, "crc": io::crc32(&d), "size": d.len(), "text": String::from_utf8(d.clone()).ok().filter(|t| t.len() < 200_000 && !t.contains('\u{0}')) }))
                .collect();
            json!({ "entries": entries, "files": a.files, "missing": a.missing.iter().map(|m| json!({ "from": m.from, "ref": m.r#ref })).collect::<Vec<_>>(), "notes": a.notes,
                "zipHead": zip_heads(&a.bytes) })
        }
        "resolveRef" => {
            let paths = ids(s, "paths").unwrap_or_default();
            json!(io::resolve_ref(st(s, "ref").unwrap_or(""), st(s, "from").unwrap_or(""), &paths))
        }
        other => return Err(format!("no op {other}")),
    })
}

/// Each local header's fields but its compressed size (the deflated bytes differ from the browser's): version, flags,
/// method, time, date, CRC, size, name length.
fn zip_heads(b: &[u8]) -> Vec<Value> {
    let mut out = Vec::new();
    let mut p = 0usize;
    let rd16 = |i: usize| b.get(i..i + 2).and_then(|x| <[u8; 2]>::try_from(x).ok()).map_or(0, |x| u16::from_le_bytes(x) as usize);
    let rd32 = |i: usize| b.get(i..i + 4).and_then(|x| <[u8; 4]>::try_from(x).ok()).map_or(0, |x| u32::from_le_bytes(x) as usize);
    while p + 30 <= b.len() && rd32(p) == 0x0403_4b50 && out.len() < 10_000 {
        let n = rd16(p + 26);
        let size = rd32(p + 18);
        out.push(json!([rd16(p + 4), rd16(p + 6), rd16(p + 10), rd16(p + 12), rd32(p + 14), rd32(p + 22), n]));
        p += 30 + n + rd16(p + 28) + size;
    }
    out
}

pub fn sheetset(rest: &[&str]) -> ExitCode {
    let Some(file) = rest.first() else {
        eprintln!("usage: cadcraft-cli sheetset <script.json>");
        return ExitCode::from(2);
    };
    let script: Value =
        match std::fs::read(file).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string())) {
            Ok(v) => v,
            Err(e) => {
                println!("{}", json!({ "$error": { "code": "io", "message": e } }));
                return ExitCode::from(1);
            }
        };
    let l = Lang::new(script.get("strings").unwrap_or(&Value::Null));
    let mut folder = io::DirFolder {
        root: std::path::PathBuf::from(script.get("folder").and_then(Value::as_str).unwrap_or(".")),
        writable: script.get("writable").and_then(Value::as_bool).unwrap_or(true),
    };
    let stamp = io::Stamp { created_ms: script.get("created").and_then(Value::as_f64), guid_sequence: true };
    let empty = Vec::new();
    let out: Vec<Value> = script
        .get("steps")
        .and_then(Value::as_array)
        .unwrap_or(&empty)
        .iter()
        .map(|s| match step(s, &mut folder, stamp, &l) {
            Ok(r) => json!({ "r": r }),
            Err(e) => json!({ "err": e }),
        })
        .collect();
    println!("{}", Value::Array(out));
    ExitCode::SUCCESS
}
