//! sheetset-io.js and sheetset-archive.js — a sheet set's drawings read and written without opening them on screen
//! (0.4 part 2, track Z): open one drawing of the set, the layouts a file has, publish the set as one PDF (title blocks
//! filled from the set in the copy that is plotted), update the drawings (title blocks and tab names written into each
//! file, saved back in its own format and version), put the sheet list on a sheet, and archive the set as one zip (the
//! set file, every drawing it uses and what they link to, with a transmittal note).
//!
//! The files come through a [`Folder`]: paths relative to the set's folder, "/" between folders (sheetset.js
//! `cleanPath`); the desktop gives a real folder, the parity harness a scratch one.

use cc_cad::{Doc, js};
use serde_json::{Value, json};

use crate::lang::Lang;
use crate::sheetset::{
    all_sheets, clean_path, field_values, fill_title_blocks, layout_name_for, place_sheet_list, refresh_sheet_list, sheet_label,
    update_sheet,
};

/// A folder on disk: every path stays inside it (no "..", no absolute path, no link out of it).
pub struct DirFolder {
    pub root: std::path::PathBuf,
    pub writable: bool,
}
impl DirFolder {
    fn at(&self, path: &str) -> Result<std::path::PathBuf, String> {
        let clean = clean_path(Some(&Value::String(path.to_string())));
        if clean.is_empty() || clean.split('/').any(|p| p == "..") {
            return Err(format!("{path}: outside the set's folder"));
        }
        let p = self.root.join(&clean);
        // a link that leads out of the folder is refused
        if let (Ok(r), Ok(c)) = (self.root.canonicalize(), p.canonicalize())
            && !c.starts_with(&r)
        {
            return Err(format!("{path}: outside the set's folder"));
        }
        Ok(p)
    }
}
impl Folder for DirFolder {
    fn name(&self) -> String {
        self.root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "folder".into())
    }
    fn read(&mut self, path: &str) -> Result<Vec<u8>, String> {
        let p = self.at(path)?;
        let meta = std::fs::metadata(&p).map_err(|e| e.to_string())?;
        if meta.len() > 2 * 1024 * 1024 * 1024 {
            return Err(format!("{path}: over 2 GB"));
        }
        std::fs::read(&p).map_err(|e| e.to_string())
    }
    fn write(&mut self, path: &str, data: &[u8]) -> Option<Result<(), String>> {
        if !self.writable {
            return None;
        }
        Some(self.at(path).and_then(|p| {
            if let Some(d) = p.parent() {
                std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
            }
            std::fs::write(&p, data).map_err(|e| e.to_string())
        }))
    }
    fn writable(&self) -> bool {
        self.writable
    }
    fn list(&mut self) -> Option<Vec<String>> {
        fn rec(dir: &std::path::Path, pre: &str, depth: usize, out: &mut Vec<String>) {
            let Ok(rd) = std::fs::read_dir(dir) else { return };
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                let Ok(ft) = e.file_type() else { continue };
                if ft.is_dir() {
                    if depth < 6 && !n.starts_with('.') {
                        rec(&e.path(), &format!("{pre}{n}/"), depth.saturating_add(1), out);
                    }
                } else if ft.is_file() && out.len() < 100_000 {
                    out.push(format!("{pre}{n}"));
                }
            }
        }
        let mut out = Vec::new();
        rec(&self.root, "", 0, &mut out);
        out.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
        Some(out)
    }
}

/// The set's folder.
pub trait Folder {
    fn name(&self) -> String;
    fn read(&mut self, path: &str) -> Result<Vec<u8>, String>;
    /// None: a read-only folder
    fn write(&mut self, _path: &str, _data: &[u8]) -> Option<Result<(), String>> {
        None
    }
    fn writable(&self) -> bool {
        false
    }
    /// every file in it ("/" paths, sorted); None when the folder cannot list
    fn list(&mut self) -> Option<Vec<String>> {
        None
    }
}

/// A drawing of the set read into a Doc of its own: the doc, its format ("dwg" | "dxf" | "dxf2000") and DWG version.
pub struct Opened {
    pub doc: Doc,
    pub format: &'static str,
    pub version: String,
}

/// `openDrawing(folder, path)`.
pub fn open_drawing(folder: &mut dyn Folder, path: &str, l: &Lang) -> Result<Opened, String> {
    let bytes = folder.read(path)?;
    let file = path.rsplit('/').next().unwrap_or(path).to_string();
    let name = match file.rfind('.') {
        Some(i) if i > 0 || file.len() > 1 => file.get(..i).unwrap_or("").to_string(),
        _ => file.clone(),
    };
    let mut doc = Doc::new();
    let lower = path.to_ascii_lowercase();
    let (format, version, mut d) = if lower.ends_with(".dxf") {
        let text = cc_io_dwg::dxf_model::decode_dxf_bytes(&bytes);
        let d = cc_io_cad::dxf_read::read_dxf(&text).map_err(|e| e.0)?;
        let v = d.get("meta").and_then(|m| m.get("version")).and_then(Value::as_str).unwrap_or("").to_string();
        (if v.is_empty() || v == "AC1009" { "dxf" } else { "dxf2000" }, v, d)
    } else {
        let tag = String::from_utf8_lossy(bytes.get(..6).unwrap_or(&bytes)).into_owned();
        let model = cc_io_dwg::read_dwg(&bytes, &cc_io_dwg::ReadOpts::default());
        let has_blocks = model.arr("blocks").is_some_and(|b| !b.is_empty());
        if !has_blocks {
            return Err(l.t("sheetset.io.unreadable", &[("file", path.to_string())]));
        }
        let d = cc_io_cad::import_dwg(&model, &tag).map_err(|e| e.0)?;
        let v = cc_io_cad::dwg_save::dwg_version_key(&tag).unwrap_or("2018").to_string();
        ("dwg", v, d)
    };
    if let Some(o) = d.as_object_mut() {
        let mut meta = o.get("meta").and_then(Value::as_object).cloned().unwrap_or_default();
        meta.insert("name".into(), json!(name));
        meta.insert("source".into(), json!(file));
        o.insert("meta".into(), Value::Object(meta));
    }
    doc.load(&d);
    doc.mark_saved();
    Ok(Opened { doc, format, version })
}

/// `layoutsIn(folder, path)`: the sheet names a file has.
pub fn layouts_in(folder: &mut dyn Folder, path: &str, l: &Lang) -> Result<Vec<String>, String> {
    Ok(open_drawing(folder, path, l)?.doc.layouts.iter().map(|x| x.name.clone()).collect())
}

/// The sheets to work on: every one, a subset's (and its subsets'), or those listed by id — in set order.
fn pick(set: &Value, subset: Option<&str>, ids: Option<&[String]>) -> Vec<Value> {
    all_sheets(set)
        .into_iter()
        .filter(|x| subset.is_none_or(|g| x.path.iter().any(|p| p.get("id").and_then(Value::as_str) == Some(g))))
        .filter(|x| ids.is_none_or(|w| w.iter().any(|i| x.sheet.get("id").and_then(Value::as_str) == Some(i.as_str()))))
        .map(|x| x.sheet)
        .collect()
}

/// What publishing said.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Published {
    pub bytes: Option<Vec<u8>>,
    pub pages: usize,
    pub names: Vec<String>,
    pub missing: Vec<String>,
    pub notes: Vec<String>,
    /// characters the PDF's font has no glyph for
    pub missing_chars: usize,
}

fn err_of(e: &str) -> String {
    e.to_string()
}

/// `publishSet(set, folder, { subset, ids, pen, mono, fields })`: one PDF in set order, a page and a bookmark per sheet.
#[allow(clippy::too_many_arguments)]
pub fn publish_set(
    set: &Value,
    folder: &mut dyn Folder,
    subset: Option<&str>,
    ids: Option<&[String]>,
    pen: f64,
    mono: bool,
    fields: bool,
    date_ms: f64,
    l: &Lang,
) -> Published {
    let todo = pick(set, subset, ids);
    let mut docs: Vec<(String, Result<Opened, String>)> = Vec::new();
    let mut lists = Vec::new();
    let mut out = Published::default();
    for sheet in todo {
        let file = sheet.get("file").and_then(Value::as_str).unwrap_or("").to_string();
        if !docs.iter().any(|(p, _)| *p == file) {
            let r = open_drawing(folder, &file, l);
            docs.push((file.clone(), r));
        }
        let label = sheet_label(&sheet);
        let Some((_, got)) = docs.iter_mut().find(|(p, _)| *p == file) else { continue };
        let doc = match got {
            Err(e) => {
                out.missing.push(label.clone());
                out.notes.push(l.t("sheetset.io.nofile", &[("sheet", label), ("file", file), ("why", err_of(e))]));
                continue;
            }
            Ok(o) => &mut o.doc,
        };
        let lay_name = sheet.get("layout").and_then(Value::as_str).unwrap_or("").to_string();
        if !doc.layouts.iter().any(|x| x.name == lay_name) {
            out.missing.push(label.clone());
            out.notes.push(l.t("sheetset.io.nolayout", &[("sheet", label), ("layout", lay_name), ("file", file)]));
            continue;
        }
        if fields {
            let v = field_values(set, &sheet, l);
            fill_title_blocks(doc, &lay_name, &v, l);
            refresh_sheet_list(doc, set, l);
        }
        let Some(lay) = doc.layouts.iter().find(|x| x.name == lay_name).cloned() else { continue };
        let sh = cc_io_cad::plot::layout_sheet(doc, &lay);
        let list = cc_io_cad::plot::display_list(doc, &sh, mono);
        lists.push(list);
        out.names.push(label);
    }
    if lists.is_empty() {
        return out;
    }
    let title = set.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    let pdf = cc_io_cad::plot::to_pdf_pages(&lists, pen, &title, &out.names, date_ms, true);
    out.pages = lists.len();
    out.missing_chars = pdf.missing;
    out.bytes = Some(pdf.bytes);
    out
}

/// How a DWG is stamped when saved: its creation time (None = now) and, for the parity harness, GUIDs from a sequence.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stamp {
    pub created_ms: Option<f64>,
    pub guid_sequence: bool,
}

/// A drawing saved in its own format and version.
fn save_as_was(o: &Opened, st: Stamp) -> Result<Vec<u8>, String> {
    match o.format {
        "dwg" => {
            let opts = cc_io_cad::dwg_save::SaveOpts {
                version: o.version.clone(),
                created_ms: st.created_ms,
                guid_sequence: st.guid_sequence,
                ..Default::default()
            };
            cc_io_cad::dwg_save::save_dwg(&o.doc, &opts).map(|s| s.bytes).map_err(|e| e.0)
        }
        "dxf2000" => cc_io_cad::dxf2000::write_dxf2000(&o.doc).map(|(t, _)| t.into_bytes()).map_err(|e| e.0),
        _ => cc_io_cad::dxf_write::write_dxf(&o.doc).map(|(t, _)| t.into_bytes()).map_err(|e| e.0),
    }
}

/// One drawing Update drawings saved.
#[derive(Clone, Debug, PartialEq)]
pub struct Updated {
    pub path: String,
    pub fields: usize,
    pub renamed: usize,
    pub list: usize,
}

/// `updateDrawings(set, folder, { fields, names })` → (the set pointing at the new tab names, the files saved, notes).
pub fn update_drawings(
    set: &Value,
    folder: &mut dyn Folder,
    fields: bool,
    names: bool,
    st: Stamp,
    l: &Lang,
) -> Result<(Value, Vec<Updated>, Vec<String>), String> {
    if !folder.writable() {
        return Err(l.s("sheetset.io.readonly"));
    }
    let mut out = set.clone();
    let mut by_file: Vec<(String, Vec<Value>)> = Vec::new();
    for x in all_sheets(set) {
        let f = x.sheet.get("file").and_then(Value::as_str).unwrap_or("").to_string();
        match by_file.iter_mut().find(|(p, _)| *p == f) {
            Some((_, v)) => v.push(x.sheet),
            None => by_file.push((f, vec![x.sheet])),
        }
    }
    let mut files = Vec::new();
    let mut notes = Vec::new();
    for (path, sheets) in by_file {
        let mut got = match open_drawing(folder, &path, l) {
            Ok(g) => g,
            Err(e) => {
                let first = sheets.first().map(sheet_label).unwrap_or_default();
                notes.push(l.t("sheetset.io.nofile", &[("sheet", first), ("file", path.clone()), ("why", e)]));
                continue;
            }
        };
        let (mut n_fields, mut renamed) = (0usize, 0usize);
        for s in &sheets {
            let want_lay = s.get("layout").and_then(Value::as_str).unwrap_or("").to_string();
            let Some(mut lay) = got.doc.layouts.iter().find(|x| x.name == want_lay).cloned() else {
                notes.push(l.t("sheetset.io.nolayout", &[("sheet", sheet_label(s)), ("layout", want_lay), ("file", path.clone())]));
                continue;
            };
            let sid = s.get("id").and_then(Value::as_str).unwrap_or("").to_string();
            if names {
                let want = layout_name_for(&out, s);
                if !want.is_empty() && want != lay.name {
                    let clash = got.doc.layouts.iter().any(|x| x.space != lay.space && x.name.to_lowercase() == want.to_lowercase());
                    if clash {
                        notes.push(l.t("sheetset.io.clash", &[("name", want), ("file", path.clone())]));
                    } else {
                        let old = lay.name.clone();
                        let list: Vec<cc_cad::doc::Layout> = got
                            .doc
                            .layouts
                            .iter()
                            .map(|x| if x.space == lay.space { cc_cad::doc::Layout { name: want.clone(), ..x.clone() } } else { x.clone() })
                            .collect();
                        let _ = got.doc.change(&l.s("sheetset.undo.rename"), |d| d.set_layouts(list));
                        if let Some(x) = got.doc.layouts.iter().find(|x| x.name == want) {
                            lay = x.clone();
                        }
                        // named views of that sheet follow it (changed in place, as the JS does, outside the change)
                        for v in got.doc.views.iter_mut() {
                            if v.layout.as_deref() == Some(old.as_str()) {
                                v.layout = Some(want.clone());
                            }
                        }
                        out = update_sheet(&out, &sid, &json!({ "layout": want }), l);
                        renamed = renamed.saturating_add(1);
                    }
                }
            }
            if fields
                && let Some(s2) = all_sheets(&out).into_iter().find(|x| x.sheet.get("id").and_then(Value::as_str) == Some(sid.as_str()))
            {
                let v = field_values(&out, &s2.sheet, l);
                n_fields = n_fields.saturating_add(fill_title_blocks(&mut got.doc, &lay.name, &v, l));
            }
        }
        let list = usize::from(refresh_sheet_list(&mut got.doc, &out, l));
        if n_fields == 0 && renamed == 0 && list == 0 {
            continue;
        }
        let data = save_as_was(&got, st)?;
        folder.write(&path, &data).unwrap_or(Err(l.s("sheetset.io.readonly")))?;
        files.push(Updated { path, fields: n_fields, renamed, list });
    }
    Ok((out, files, notes))
}

/// `addSheetList(set, folder, sheet)`: the sheet list table on that sheet, saved.
pub fn add_sheet_list(set: &Value, folder: &mut dyn Folder, sheet: &Value, st: Stamp, l: &Lang) -> Result<String, String> {
    if !folder.writable() {
        return Err(l.s("sheetset.io.readonly"));
    }
    let file = sheet.get("file").and_then(Value::as_str).unwrap_or("").to_string();
    let lay = sheet.get("layout").and_then(Value::as_str).unwrap_or("").to_string();
    let mut got = open_drawing(folder, &file, l)?;
    if !place_sheet_list(&mut got.doc, &lay, set, None, l) {
        return Err(l.t("sheetset.io.nolayout", &[("sheet", sheet_label(sheet)), ("layout", lay), ("file", file)]));
    }
    let data = save_as_was(&got, st)?;
    folder.write(&file, &data).unwrap_or(Err(l.s("sheetset.io.readonly")))?;
    Ok(file)
}

// ---------------------------------------------------------------------------- sheetset-archive.js

/// `linksOf(model)`: the linked drawings (xref paths as written) and pictures (IMAGEDEF file names) of a cc-dwg /
/// dxf-model.js drawing model.
pub fn links_of(model: &cc_io_dwg::value::Obj) -> (Vec<String>, Vec<String>) {
    use cc_io_dwg::value::V;
    let text = |v: &V| match v {
        V::Str(s) => s.clone(),
        V::Num(n) => js::num_to_string(*n),
        V::Bool(b) => b.to_string(),
        _ => String::new(),
    };
    let mut xrefs: Vec<String> = Vec::new();
    let mut images: Vec<String> = Vec::new();
    for b in model.arr("blocks").into_iter().flatten() {
        let Some(b) = b.as_obj() else { continue };
        let flags = b.num("flags").unwrap_or(0.0);
        let flags = if flags.is_finite() { flags.trunc() } else { 0.0 };
        // `b.flags & 4` on a 32-bit integer
        #[allow(clippy::cast_possible_truncation)]
        #[allow(clippy::cast_sign_loss)]
        let bit = (flags.rem_euclid(4_294_967_296.0) as u64) & 4 != 0;
        if bit
            && b.truthy("xrefPath")
            && let Some(v) = b.get("xrefPath")
        {
            let s = text(v);
            if !xrefs.contains(&s) {
                xrefs.push(s);
            }
        }
    }
    for o in model.arr("objects").into_iter().flatten() {
        let Some(o) = o.as_obj() else { continue };
        if o.str("type") == Some("IMAGEDEF")
            && o.truthy("fileName")
            && let Some(v) = o.get("fileName")
        {
            let s = text(v);
            if !images.contains(&s) {
                images.push(s);
            }
        }
    }
    (xrefs, images)
}

/// `resolveRef(ref, fromFile, paths)`: the folder path a link means — relative to the drawing that names it, else the
/// same file name anywhere in the folder (an absolute path from another computer), nearest first.
pub fn resolve_ref(r: &str, from_file: &str, paths: &[String]) -> Option<String> {
    let raw = js::trim(&r.replace('\\', "/")).to_string();
    if raw.is_empty() {
        return None;
    }
    let dir: Vec<&str> = match from_file.rfind('/') {
        Some(i) => from_file.get(..i).unwrap_or("").split('/').collect(),
        None => Vec::new(),
    };
    let lower = raw.to_ascii_lowercase();
    let abs = lower.starts_with('/')
        || (lower.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
            && lower.as_bytes().get(1) == Some(&b':')
            && lower.as_bytes().get(2) == Some(&b'/'));
    let url = {
        let rest: String = lower.chars().skip(1).take_while(|c| c.is_ascii_lowercase()).collect();
        lower.chars().next().is_some_and(|c| c.is_ascii_lowercase())
            && lower.get(rest.len().saturating_add(1)..).is_some_and(|t| t.starts_with("://"))
    };
    if !abs && !url {
        let mut parts: Vec<&str> = dir.clone();
        let mut above = false;
        for p in raw.split('/') {
            if p.is_empty() || p == "." {
                continue;
            }
            if p != ".." {
                parts.push(p);
            } else if !parts.is_empty() {
                parts.pop();
            } else {
                above = true;
                break;
            }
        }
        let j = parts.join("/");
        if !above && paths.contains(&j) {
            return Some(j);
        }
    }
    let name = raw.rsplit('/').next().unwrap_or("").to_lowercase();
    let mut same: Vec<&String> = paths.iter().filter(|p| p.rsplit('/').next().unwrap_or("").to_lowercase() == name).collect();
    if same.is_empty() {
        return None;
    }
    let score = |p: &str| {
        let a: Vec<&str> = p.split('/').collect();
        let a = a.get(..a.len().saturating_sub(1)).unwrap_or(&[]);
        let mut k = 0usize;
        while k < a.len() && k < dir.len() && a.get(k) == dir.get(k) {
            k = k.saturating_add(1);
        }
        k
    };
    same.sort_by(|a, b| score(b).cmp(&score(a)).then(a.encode_utf16().count().cmp(&b.encode_utf16().count())));
    same.first().map(|s| (*s).clone())
}

/// Reads a drawing's model for its links (path → cc-dwg / dxf-model.js model).
pub type ReadModel = dyn Fn(&[u8], &str) -> Result<cc_io_dwg::value::Obj, String>;

/// A missing link: what named it and how.
#[derive(Clone, Debug, PartialEq)]
pub struct Missing {
    pub from: String,
    pub r#ref: String,
}

/// What archiving made.
#[derive(Clone, Debug, PartialEq)]
pub struct Archived {
    pub bytes: Vec<u8>,
    pub files: Vec<String>,
    pub missing: Vec<Missing>,
    pub notes: Vec<String>,
}

/// The date and time a zip entry carries (MS-DOS fields, local time).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DosTime {
    pub time: u16,
    pub date: u16,
}

/// `archiveSet(set, folder, { setFile, readModel, setText })`. `today` is the transmittal note's date (yyyy-mm-dd,
/// UTC as toISOString gives it); `read_model` reads a drawing's links (cc-dwg / dxf-model.js model).
#[allow(clippy::too_many_arguments)]
pub fn archive_set(
    set: &Value,
    folder: &mut dyn Folder,
    set_file: &str,
    set_text: Option<&str>,
    read_model: &ReadModel,
    today: &str,
    when: DosTime,
    l: &Lang,
) -> Archived {
    let paths = folder.list().unwrap_or_default();
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    let mut missing: Vec<Missing> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut uniq: Vec<String> = Vec::new();
    for x in all_sheets(set) {
        let f = x.sheet.get("file").and_then(Value::as_str).unwrap_or("").to_string();
        if !uniq.contains(&f) {
            uniq.push(f);
        }
    }
    let mut queue: std::collections::VecDeque<(String, String, String)> =
        uniq.into_iter().map(|p| (p.clone(), set_file.to_string(), p)).collect();
    if let Some(t) = set.get("template").and_then(|t| t.get("file")).and_then(Value::as_str).filter(|s| !s.is_empty()) {
        queue.push_back((t.to_string(), set_file.to_string(), t.to_string()));
    }
    let mut seen: Vec<String> = Vec::new();
    let mut steps = 0usize;
    while let Some((p, from, named)) = queue.pop_front() {
        steps = steps.saturating_add(1);
        if steps > 100_000 {
            break;
        }
        if seen.contains(&p) {
            continue;
        }
        seen.push(p.clone());
        let bytes = match folder.read(&p) {
            Ok(b) => b,
            Err(_) => {
                missing.push(Missing { from, r#ref: named });
                continue;
            }
        };
        let lower = p.to_ascii_lowercase();
        let drawing = lower.ends_with(".dwg") || lower.ends_with(".dxf");
        let model = if drawing { Some(read_model(&bytes, &p)) } else { None };
        match entries.iter_mut().find(|(k, _)| *k == p) {
            Some(slot) => slot.1 = bytes,
            None => entries.push((p.clone(), bytes)),
        }
        let Some(model) = model else { continue };
        let model = match model {
            Ok(m) => m,
            Err(e) => {
                notes.push(l.t("sheetset.archive.unread", &[("file", p.clone()), ("why", e)]));
                continue;
            }
        };
        let (xrefs, images) = links_of(&model);
        for r in xrefs.iter().chain(images.iter()) {
            let Some(hit) = resolve_ref(r, &p, &paths) else {
                missing.push(Missing { from: p.clone(), r#ref: r.clone() });
                continue;
            };
            if xrefs.contains(r) {
                queue.push_back((hit, p.clone(), r.clone()));
            } else if !entries.iter().any(|(k, _)| *k == hit) {
                match folder.read(&hit) {
                    Ok(b) => entries.push((hit, b)),
                    Err(_) => missing.push(Missing { from: p.clone(), r#ref: r.clone() }),
                }
            }
        }
    }
    let mut keys: Vec<String> = entries.iter().map(|(k, _)| k.clone()).collect();
    keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    let list: Vec<String> = std::iter::once(set_file.to_string()).chain(keys).collect();
    let mut lines: Vec<String> = vec![
        l.t("sheetset.archive.head", &[("name", set.get("name").map(js::to_js_string).unwrap_or_default()), ("date", today.to_string())]),
        String::new(),
        l.t("sheetset.archive.files", &[("n", list.len().to_string())]),
    ];
    lines.extend(list.iter().map(|f| format!("  {f}")));
    lines.push(String::new());
    if missing.is_empty() {
        lines.push(l.s("sheetset.archive.complete"));
    } else {
        lines.push(l.t("sheetset.archive.missing", &[("n", missing.len().to_string())]));
        lines.extend(
            missing
                .iter()
                .map(|m| format!("  {}", l.t("sheetset.archive.missing.row", &[("ref", m.r#ref.clone()), ("from", m.from.clone())]))),
        );
    }
    lines.push(String::new());
    let note = lines.join("\r\n");
    let mut zip_entries: Vec<(String, Vec<u8>)> = Vec::new();
    if let Some(t) = set_text {
        zip_entries.push((set_file.to_string(), t.as_bytes().to_vec()));
    }
    for (name, data) in entries {
        let c = clean_path(Some(&Value::String(name.clone())));
        zip_entries.push((if c.is_empty() { name } else { c }, data));
    }
    zip_entries.push((l.s("sheetset.archive.notefile"), note.into_bytes()));
    Archived { bytes: zip(&zip_entries, when), files: list, missing, notes }
}

fn crc_table() -> [u32; 256] {
    let mut tb = [0u32; 256];
    for (n, slot) in tb.iter_mut().enumerate() {
        let mut c = u32::try_from(n).unwrap_or(0);
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *slot = c;
    }
    tb
}

/// `crc32(bytes)`.
pub fn crc32(b: &[u8]) -> u32 {
    let tb = crc_table();
    let mut c = 0xffff_ffffu32;
    for x in b {
        c = tb.get(((c ^ u32::from(*x)) & 0xff) as usize).copied().unwrap_or(0) ^ (c >> 8);
    }
    c ^ 0xffff_ffff
}

/// Deflate (raw), kept only when smaller (sheetset-archive.js `deflateRaw`).
fn deflate_raw(b: &[u8]) -> Option<Vec<u8>> {
    let out = cc_io_cad::plot::deflate_raw(b);
    (out.len() < b.len()).then_some(out)
}

/// `zip(entries)`: stored or deflated entries, UTF-8 names, CRC-32, a central directory (PKWARE APPNOTE 6.3).
pub fn zip(entries: &[(String, Vec<u8>)], when: DosTime) -> Vec<u8> {
    let mut parts: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    let mut off: u32 = 0;
    let u16le = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());
    let u32le = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
    for (name, data) in entries {
        let nb = name.as_bytes();
        let crc = crc32(data);
        let packed = deflate_raw(data);
        let (body, method) = match &packed {
            Some(p) => (p.as_slice(), 8u16),
            None => (data.as_slice(), 0u16),
        };
        let nlen = u16::try_from(nb.len()).unwrap_or(u16::MAX);
        let blen = u32::try_from(body.len()).unwrap_or(u32::MAX);
        let dlen = u32::try_from(data.len()).unwrap_or(u32::MAX);
        u32le(&mut parts, 0x0403_4b50);
        u16le(&mut parts, 20);
        u16le(&mut parts, 0x0800);
        u16le(&mut parts, method);
        u16le(&mut parts, when.time);
        u16le(&mut parts, when.date);
        u32le(&mut parts, crc);
        u32le(&mut parts, blen);
        u32le(&mut parts, dlen);
        u16le(&mut parts, nlen);
        u16le(&mut parts, 0);
        parts.extend_from_slice(nb);
        parts.extend_from_slice(body);
        u32le(&mut central, 0x0201_4b50);
        u16le(&mut central, 20);
        u16le(&mut central, 20);
        u16le(&mut central, 0x0800);
        u16le(&mut central, method);
        u16le(&mut central, when.time);
        u16le(&mut central, when.date);
        u32le(&mut central, crc);
        u32le(&mut central, blen);
        u32le(&mut central, dlen);
        u16le(&mut central, nlen);
        central.extend_from_slice(&[0u8; 12]);
        u32le(&mut central, off);
        central.extend_from_slice(nb);
        off = off.saturating_add(30).saturating_add(u32::from(nlen)).saturating_add(blen);
    }
    let cd = u32::try_from(central.len()).unwrap_or(u32::MAX);
    let n = u16::try_from(entries.len()).unwrap_or(u16::MAX);
    let mut out = parts;
    out.extend_from_slice(&central);
    u32le(&mut out, 0x0605_4b50);
    u16le(&mut out, 0);
    u16le(&mut out, 0);
    u16le(&mut out, n);
    u16le(&mut out, n);
    u32le(&mut out, cd);
    u32le(&mut out, off);
    u16le(&mut out, 0);
    out
}

/// The zip's entries back (stored and deflated): (name, data) — for checks.
pub fn unzip(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut p = 0usize;
    let rd16 = |i: usize| {
        bytes.get(i..i.saturating_add(2)).and_then(|b| <[u8; 2]>::try_from(b).ok()).map_or(0, |b| usize::from(u16::from_le_bytes(b)))
    };
    let rd32 = |i: usize| {
        bytes.get(i..i.saturating_add(4)).and_then(|b| <[u8; 4]>::try_from(b).ok()).map_or(0, |b| u32::from_le_bytes(b) as usize)
    };
    while p.saturating_add(30) <= bytes.len() && rd32(p) == 0x0403_4b50 && out.len() < 100_000 {
        let method = rd16(p.saturating_add(8));
        let size = rd32(p.saturating_add(18));
        let n = rd16(p.saturating_add(26));
        let x = rd16(p.saturating_add(28));
        let s0 = p.saturating_add(30);
        let name = String::from_utf8_lossy(bytes.get(s0..s0.saturating_add(n)).unwrap_or(&[])).into_owned();
        let b0 = s0.saturating_add(n).saturating_add(x);
        let body = bytes.get(b0..b0.saturating_add(size)).unwrap_or(&[]);
        let data = if method == 8 { cc_io_cad::plot::inflate_raw(body).unwrap_or_default() } else { body.to_vec() };
        out.push((name, data));
        p = b0.saturating_add(size);
    }
    out
}
