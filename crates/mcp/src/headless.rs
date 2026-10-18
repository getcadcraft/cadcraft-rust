//! The headless tools — 0.5 tools/cli/headless/tools.mjs (and cad.mjs, takeoff.mjs, markup.mjs, ifc.mjs) on the Rust
//! engines: what `cadcraft-cli mcp` serves over stdio. Files only inside the confinement roots ([`crate::confine`]);
//! open files are kept by handle (`d1`, `d2`, …; 16 at most).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use cc_cad::{Doc, js};
use cc_cad_geom::geom::Bounds;
use cc_engine::{Engine, Host};
use cc_geom::P2;
use serde_json::{Map, Value, json};

use crate::cad;
use crate::confine::{Roots, write_file_atomic};
use crate::defs::headless_defs;
use crate::rpc::{CallCtx, ToolDef, ToolError, ToolOut, Tools};

/// Files open at once.
pub const MAX_OPEN: usize = 16;
/// The largest file a tool reads.
pub const MAX_FILE: u64 = 500 * 1024 * 1024;

/// The headless host (cad.mjs): what the screen would answer, without one.
struct HeadlessHost {
    cursor: Option<P2>,
    selection: Vec<u64>,
    prompt: String,
}

impl Host for HeadlessHost {
    fn print(&mut self, _s: &str) {}
    fn prompt(&mut self, s: &str) {
        self.prompt = s.to_string();
    }
    fn cursor(&self) -> Option<P2> {
        Some(self.cursor.unwrap_or(P2::new(0.0, 0.0)))
    }
    fn selection(&self) -> Vec<u64> {
        self.selection.clone()
    }
    fn set_selection(&mut self, ids: &[u64]) {
        self.selection = ids.to_vec();
    }
    fn pick_at(&self, doc: &Doc, p: P2) -> Option<u64> {
        let mut best = None;
        // cad.mjs: the tolerance follows the drawing's extents at each pick
        let mut bd = pick_tolerance(doc);
        for e in doc.entities() {
            if !doc.editable(e) {
                continue;
            }
            let d = cc_cad_geom::pick::dist_to_entity(e, p, Some(&doc.blocks));
            if d < bd {
                bd = d;
                best = e.id;
            }
        }
        best
    }
    fn pick_window(&self, doc: &Doc, w: &Bounds, crossing: bool) -> Vec<u64> {
        doc.entities()
            .filter(|e| doc.editable(e) && cc_cad_geom::pick::in_window(e, w, crossing, Some(&doc.blocks)))
            .filter_map(|e| e.id)
            .collect()
    }
    fn preview(&mut self, _ents: &[cc_cad::Entity]) {}
    fn zoom_extents(&mut self) {}
    fn zoom_window(&mut self, _a: P2, _b: P2, _exact: bool) {}
}

/// cad.mjs `pickTolerance`: 0.5 % of the extents' larger side (100 when empty), at least 1e-6.
fn pick_tolerance(doc: &Doc) -> f64 {
    let size = cad::extents_of(doc).map_or(100.0, |b| (b.max_x - b.min_x).max(b.max_y - b.min_y));
    (size * 0.005).max(1e-6)
}

/// An open drawing.
pub struct CadDoc {
    pub eng: Engine,
    pub report: Option<Value>,
    pub format: Option<String>,
}

impl CadDoc {
    fn new(doc: Doc) -> Self {
        let host = HeadlessHost { cursor: None, selection: Vec::new(), prompt: String::new() };
        CadDoc { eng: Engine::new(doc, Box::new(host), cad::lang()), report: None, format: None }
    }
}

pub use crate::takeoff::TakeoffDoc;

enum Open {
    Cad(Box<CadDoc>),
    Takeoff(Box<TakeoffDoc>),
    Ifc(Box<crate::ifc::IfcDoc>),
}

impl Open {
    fn kind(&self) -> &'static str {
        match self {
            Open::Cad(_) => "cad",
            Open::Takeoff(_) => "takeoff",
            Open::Ifc(_) => "ifc",
        }
    }
}

struct Entry {
    open: Open,
    path: Option<PathBuf>,
}

/// The headless session: the roots, the open files, the files this session opened or wrote.
pub struct Headless {
    pub roots: Roots,
    pub undo_prefix: String,
    docs: Vec<(String, Entry)>,
    seq: u64,
    mine: HashSet<PathBuf>,
    /// the clock for exports that carry a date (milliseconds since 1970)
    pub now: Box<dyn Fn() -> f64>,
}

impl std::fmt::Debug for Headless {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Headless").field("roots", &self.roots).field("open", &self.docs.len()).finish()
    }
}

fn what(kind: &str) -> &'static str {
    match kind {
        "cad" => "a drawing",
        "ifc" => "an IFC model",
        _ => "a takeoff",
    }
}

/// The lower-case extension of a name.
pub fn ext_of(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    match base.rsplit_once('.') {
        Some((_, e)) if !e.is_empty() && e.bytes().all(|b| b.is_ascii_alphanumeric()) => e.to_ascii_lowercase(),
        _ => String::new(),
    }
}

fn base_name(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

const DWG_TAGS: [(&str, &str); 10] = [
    ("AC1012", "R13"),
    ("AC1014", "R14"),
    ("AC1015", "2000"),
    ("AC1018", "2004"),
    ("AC1021", "2007"),
    ("AC1024", "2010"),
    ("AC1027", "2013"),
    ("AC1032", "2018"),
    ("AC1009", "R12"),
    ("AC1006", "R10"),
];
fn dwg_tag(t: &str) -> Option<&'static str> {
    DWG_TAGS.iter().find(|(k, _)| *k == t).map(|(_, v)| *v)
}

/// cad.mjs `readDrawing`: a drawing file → doc.load input + a format label.
pub fn read_drawing(bytes: &[u8], name: &str) -> Result<(Value, String), String> {
    match ext_of(name).as_str() {
        "dxf" => {
            let text = cc_io_dwg::dxf_model::decode_dxf_bytes(bytes);
            if text.starts_with("AutoCAD Binary DXF") {
                return Err("Binary DXF is not read; save it as ASCII DXF.".into());
            }
            let drawing = cc_io_cad::dxf_read::read_dxf(&text).map_err(|e| e.0)?;
            let v = drawing.get("meta").and_then(|m| m.get("version")).and_then(Value::as_str).unwrap_or("").to_string();
            let mut f = "DXF".to_string();
            if let Some(t) = dwg_tag(&v) {
                f.push(' ');
                f.push_str(t);
            }
            if !v.is_empty() {
                f.push_str(&format!(" ({v})"));
            }
            Ok((drawing, f))
        }
        "dwg" => {
            let tag = String::from_utf8_lossy(bytes.get(..6).unwrap_or(bytes)).into_owned();
            let model = std::panic::catch_unwind(|| cc_io_dwg::read_dwg(bytes, &cc_io_dwg::ReadOpts::default())).ok();
            let ok = model.as_ref().is_some_and(|m| m.get("blocks").and_then(|b| b.as_arr()).is_some_and(|a| !a.is_empty()));
            let Some(model) = model.filter(|_| ok) else {
                return Err(format!(
                    "This drawing could not be read. It may be damaged, or saved in a version the reader does not handle yet ({}).",
                    if tag.is_empty() { "unknown" } else { &tag }
                ));
            };
            let drawing = cc_io_cad::import_dwg(&model, &tag).map_err(|e| e.0)?;
            let f = format!("DWG{} ({tag})", dwg_tag(&tag).map(|t| format!(" {t}")).unwrap_or_default());
            Ok((drawing, f))
        }
        _ => Err(format!("CadCraft opens DWG and DXF drawings; \"{name}\" is neither.")),
    }
}

/// cad.mjs `CadSession.open`: the drawing loaded with its name, source, format and size in `meta`.
pub fn open_drawing(bytes: &[u8], name: &str) -> Result<CadDoc, String> {
    let (mut drawing, format) = read_drawing(bytes, name)?;
    let report = drawing.get("report").filter(|r| js::truthy(Some(r))).cloned();
    let base = base_name(name).to_string();
    let stem = match base.rsplit_once('.') {
        Some((a, _)) => a.to_string(),
        None => base.clone(),
    };
    if let Some(o) = drawing.as_object_mut() {
        let mut meta = o.get("meta").and_then(Value::as_object).cloned().unwrap_or_default();
        meta.insert("name".into(), Value::String(stem));
        meta.insert("source".into(), Value::String(base));
        meta.insert("format".into(), Value::String(format.clone()));
        meta.insert("size".into(), json!(bytes.len()));
        o.insert("meta".into(), Value::Object(meta));
    }
    let model3d = drawing.get("model3d").and_then(Value::as_str).map(str::to_string);
    let mut doc = Doc::new();
    doc.load(&drawing);
    // doc.js load → model3d.js fromFile: other programs' 3D faces come into the 3D tab, and the read report says so
    let mut report = report;
    if let Some(note) = cad::loose_faces_note(&doc, model3d.as_deref()) {
        let r = report.get_or_insert_with(|| json!({"notes": []}));
        if let Some(n) = r.get_mut("notes").and_then(Value::as_array_mut) {
            n.extend(note.into_iter().map(Value::String));
        } else if let Some(o) = r.as_object_mut() {
            o.insert("notes".into(), json!(note));
        }
    }
    let mut c = CadDoc::new(doc);
    c.report = report;
    c.format = Some(format);
    Ok(c)
}

/// tools.mjs `targetFormat`: (format, version) for a target file name.
fn target_format(out: &str, format: Option<&str>, version: Option<&str>) -> Result<(String, String), String> {
    let ext = ext_of(out);
    if ext != "dwg" && ext != "dxf" {
        return Err("Drawings are written as .dwg or .dxf.".into());
    }
    let f = format.map(str::to_string).unwrap_or_else(|| if ext == "dxf" { "dxf2000".into() } else { "dwg".into() });
    if (f == "dwg") != (ext == "dwg") {
        return Err(format!("Format {f} is written to a .{} file.", if f == "dwg" { "dwg" } else { "dxf" }));
    }
    Ok((f, version.unwrap_or("2018").to_string()))
}

/// cad.mjs `save(format, {version})` → (bytes, notes, format label).
fn save_drawing(doc: &Doc, format: &str, version: &str) -> Result<(Vec<u8>, Value, String), String> {
    let notes = |r: &Value| r.get("notes").cloned().unwrap_or_else(|| json!([]));
    match format {
        "dwg" => {
            let v = cc_io_cad::dwg_save::dwg_version_key(version).ok_or("DWG versions: 2018, 2013, 2010, 2007, 2004, 2000.")?;
            let opts = cc_io_cad::dwg_save::SaveOpts { version: v.to_string(), ..Default::default() };
            let s = cc_io_cad::dwg_save::save_dwg(doc, &opts).map_err(|e| e.0)?;
            Ok((s.bytes, notes(&s.report), format!("DWG {v}")))
        }
        "dxf2000" => {
            let (t, r) = cc_io_cad::dxf2000::write_dxf2000(doc).map_err(|e| e.0)?;
            Ok((t.into_bytes(), notes(&r), "DXF 2000".into()))
        }
        "dxf" => {
            let (t, r) = cc_io_cad::dxf_write::write_dxf(doc).map_err(|e| e.0)?;
            Ok((t.into_bytes(), notes(&r), "DXF R12".into()))
        }
        _ => Err("Formats: dwg, dxf (R12), dxf2000.".into()),
    }
}

/// `v[k]` without indexing (missing = null).
fn g<'a>(v: &'a Value, k: &str) -> &'a Value {
    v.get(k).unwrap_or(&Value::Null)
}

fn arg_str<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    a.get(k).and_then(Value::as_str)
}
fn arg_bool(a: &Value, k: &str) -> bool {
    a.get(k).and_then(Value::as_bool).unwrap_or(false)
}
fn arg_u64(a: &Value, k: &str, d: u64) -> u64 {
    a.get(k).and_then(Value::as_u64).unwrap_or(d)
}
fn path_str(p: &Path) -> String {
    p.display().to_string()
}

impl Headless {
    pub fn new(roots: Roots) -> Self {
        Headless {
            roots,
            undo_prefix: "Assistant: ".into(),
            docs: Vec::new(),
            seq: 0,
            mine: HashSet::new(),
            now: Box::new(crate::rpc::now_ms),
        }
    }

    fn read_file(&self, p: &str) -> Result<(PathBuf, Vec<u8>), String> {
        let real = self.roots.read(p).map_err(|e| e.message)?;
        let size = std::fs::metadata(&real).map(|m| m.len()).unwrap_or(0);
        if size > MAX_FILE {
            return Err(format!(
                "{} is {} MB; the limit is {} MB.",
                base_name(&path_str(&real)),
                (size as f64 / 1_048_576.0).round(),
                MAX_FILE / 1_048_576
            ));
        }
        let bytes = std::fs::read(&real).map_err(|e| format!("{}: {e}", real.display()))?;
        Ok((real, bytes))
    }

    fn keep(&mut self, open: Open, path: Option<PathBuf>) -> Result<String, String> {
        if self.docs.len() >= MAX_OPEN {
            return Err(format!("{MAX_OPEN} files are open. Close one with doc_close first."));
        }
        self.seq = self.seq.saturating_add(1);
        let id = format!("d{}", self.seq);
        if let Some(p) = &path {
            self.mine.insert(p.clone());
        }
        self.docs.push((id.clone(), Entry { open, path }));
        Ok(id)
    }

    fn entry(&mut self, id: &str, kind: Option<&str>) -> Result<&mut Entry, String> {
        let Some((_, e)) = self.docs.iter_mut().find(|(k, _)| k == id) else {
            let tool = match kind {
                Some("cad") => "drawing_open",
                Some("ifc") => "ifc_open",
                _ => "takeoff_open",
            };
            return Err(format!("No open file {id}. Open one first ({tool})."));
        };
        if let Some(k) = kind
            && e.open.kind() != k
        {
            return Err(format!("{id} is {}, not {}.", what(e.open.kind()), what(k)));
        }
        Ok(e)
    }

    fn cad(&mut self, id: &str) -> Result<&mut CadDoc, String> {
        match &mut self.entry(id, Some("cad"))?.open {
            Open::Cad(c) => Ok(c),
            _ => Err(format!("{id} is not a drawing.")),
        }
    }

    fn write(&mut self, p: &str, bytes: &[u8], overwrite: bool, ext: Option<&str>) -> Result<PathBuf, String> {
        let real = self.roots.write(p, overwrite, &self.mine).map_err(|e| e.message)?;
        if let Some(x) = ext
            && ext_of(&path_str(&real)) != x
        {
            return Err(format!("The file name must end in .{x}."));
        }
        write_file_atomic(&real, bytes)?;
        self.mine.insert(real.clone());
        Ok(real)
    }

    fn run_cad(&mut self, id: &str, input: &Value, select: Option<Vec<u64>>, label: Option<&str>) -> Result<Value, String> {
        let prefix = self.undo_prefix.clone();
        let c = self.cad(id)?;
        let r = cad::run_input(&mut c.eng, input, select.as_deref(), label, &prefix);
        Ok(trim_run(r))
    }

    fn tool(&mut self, name: &str, a: &Value) -> Result<Value, String> {
        match name {
            "files_list" => {
                let exts: Option<Vec<String>> = a
                    .get("extensions")
                    .and_then(Value::as_array)
                    .map(|v| v.iter().filter_map(|x| x.as_str().map(str::to_string)).collect());
                let (folder, files, truncated) =
                    crate::confine::list_files(&self.roots, arg_str(a, "folder"), exts.as_deref(), arg_bool(a, "recursive"), 2000)
                        .map_err(|e| e.message)?;
                let files: Vec<Value> =
                    files.into_iter().map(|f| json!({"path": f.path, "size": f.size, "modified": f.modified})).collect();
                let folders: Vec<String> = self.roots.list().iter().map(|p| path_str(p)).collect();
                Ok(json!({"folder": path_str(&folder), "folders": folders, "files": files, "truncated": truncated}))
            }
            "drawing_open" => {
                let (real, bytes) = self.read_file(arg_str(a, "path").unwrap_or(""))?;
                let c = open_drawing(&bytes, &path_str(&real))?;
                let dsc = cad::describe_drawing(&c.eng.doc(), c.report.as_ref(), c.format.as_deref());
                let doc = self.keep(Open::Cad(Box::new(c)), Some(real.clone()))?;
                let sheets: Vec<Value> =
                    g(&dsc, "layouts").as_array().map(|l| l.iter().map(|x| g(x, "name").clone()).collect()).unwrap_or_default();
                let notes: Vec<Value> = g(&dsc, "notes").as_array().map(|n| n.iter().take(10).cloned().collect()).unwrap_or_default();
                let mut o = Map::new();
                o.insert("doc".into(), json!(doc));
                o.insert("path".into(), json!(path_str(&real)));
                if let Some(n) = dsc.get("name") {
                    o.insert("name".into(), n.clone());
                }
                o.insert("format".into(), g(&dsc, "format").clone());
                o.insert("objects".into(), g(&dsc, "objects").clone());
                o.insert("layers".into(), json!(g(&dsc, "layers").as_array().map_or(0, Vec::len)));
                o.insert("sheets".into(), Value::Array(sheets));
                o.insert("units".into(), g(&dsc, "units").clone());
                o.insert("notes".into(), Value::Array(notes));
                Ok(Value::Object(o))
            }
            "drawing_new" => {
                let mut doc = Doc::new();
                doc.load(&json!({"meta": {"name": "Drawing1"}}));
                doc.meta.insert("name".into(), a.get("name").cloned().unwrap_or_else(|| json!("Drawing1")));
                let id = self.keep(Open::Cad(Box::new(CadDoc::new(doc))), None)?;
                Ok(json!({"doc": id}))
            }
            "drawing_describe" => {
                let c = self.cad(arg_str(a, "doc").unwrap_or(""))?;
                Ok(cad::describe_drawing(&c.eng.doc(), c.report.as_ref(), c.format.as_deref()))
            }
            "objects_list" => {
                let c = self.cad(arg_str(a, "doc").unwrap_or(""))?;
                cad::list_objects(&c.eng.doc(), a)
            }
            "object_get" => {
                let id = arg_u64(a, "id", 0);
                let c = self.cad(arg_str(a, "doc").unwrap_or(""))?;
                cad::object_in_full(&c.eng.doc(), id).ok_or_else(|| format!("There is no object {id}."))
            }
            "drawing_check" => {
                let c = self.cad(arg_str(a, "doc").unwrap_or(""))?;
                cad::check_drawing(&c.eng.doc(), a.get("rules").unwrap_or(&Value::Null))
            }
            "command_run" => {
                let select: Option<Vec<u64>> =
                    a.get("select").and_then(Value::as_array).map(|v| v.iter().filter_map(Value::as_u64).collect());
                self.run_cad(arg_str(a, "doc").unwrap_or(""), a.get("input").unwrap_or(&Value::Null), select, None)
            }
            "script_run" => {
                let id = arg_str(a, "doc").unwrap_or("").to_string();
                self.cad(&id)?;
                if arg_str(a, "language") == Some("lisp") {
                    return Err("This CadCraft build has no LISP.".into());
                }
                let lines: Vec<Value> = cad::script_lines(arg_str(a, "script").unwrap_or("")).into_iter().map(Value::String).collect();
                self.run_cad(&id, &Value::Array(lines), None, Some("SCRIPT"))
            }
            "undo" => {
                let prefix = self.undo_prefix.clone();
                let (steps, any) = (arg_u64(a, "steps", 1), arg_bool(a, "any"));
                let c = self.cad(arg_str(a, "doc").unwrap_or(""))?;
                let undone = cad::undo_steps(&mut c.eng.doc_mut(), steps, any, &prefix);
                Ok(json!({"undone": undone}))
            }
            "drawing_save" => {
                let id = arg_str(a, "doc").unwrap_or("").to_string();
                let own = self.entry(&id, Some("cad"))?.path.clone();
                let target = match arg_str(a, "path") {
                    Some(p) => p.to_string(),
                    None => own.as_ref().map(|p| path_str(p)).ok_or("This drawing has no file yet: give a path.")?,
                };
                let (format, version) = target_format(&target, arg_str(a, "format"), arg_str(a, "version"))?;
                let (bytes, notes, label) = {
                    let c = self.cad(&id)?;
                    save_drawing(&c.eng.doc(), &format, &version)?
                };
                let real = self.write(&target, &bytes, arg_bool(a, "overwrite"), None)?;
                self.cad(&id)?.eng.doc_mut().mark_saved();
                if let Ok(e) = self.entry(&id, Some("cad"))
                    && e.path.is_none()
                {
                    e.path = Some(real.clone());
                }
                Ok(json!({"path": path_str(&real), "bytes": bytes.len(), "format": label, "notes": notes}))
            }
            "drawing_convert" => {
                let (real, bytes) = self.read_file(arg_str(a, "path").unwrap_or(""))?;
                let c = open_drawing(&bytes, &path_str(&real))?;
                let out_arg = arg_str(a, "out").unwrap_or("").to_string();
                let (format, version) = target_format(&out_arg, arg_str(a, "format"), arg_str(a, "version"))?;
                let (out_bytes, notes, label) = save_drawing(&c.eng.doc(), &format, &version)?;
                let objects = c.eng.doc().count();
                let out = self.write(&out_arg, &out_bytes, arg_bool(a, "overwrite"), None)?;
                Ok(
                    json!({"from": path_str(&real), "out": path_str(&out), "bytes": out_bytes.len(), "format": label, "objects": objects, "notes": notes}),
                )
            }
            "drawing_plot" => {
                self.cad(arg_str(a, "doc").unwrap_or(""))?;
                if ext_of(arg_str(a, "out").unwrap_or("")) != "pdf" {
                    return Err("The plot is written to a .pdf file.".into());
                }
                Err("drawing_plot is not available in this version of CadCraft yet: its PDF plotting (plot.js) has not been ported. Use drawing_save or drawing_convert, or plot from the CadCraft app.".into())
            }
            "doc_close" => {
                let id = arg_str(a, "doc").unwrap_or("").to_string();
                self.entry(&id, None)?;
                self.docs.retain(|(k, _)| *k != id);
                Ok(json!({"closed": id}))
            }
            "ifc_open" => {
                let (real, bytes) = self.read_file(arg_str(a, "path").unwrap_or(""))?;
                let d = crate::ifc::IfcDoc::open(bytes, &path_str(&real))?;
                let summary = d.summary();
                let id = self.keep(Open::Ifc(Box::new(d)), Some(real))?;
                let mut o = Map::new();
                o.insert("doc".into(), json!(id));
                if let Value::Object(s) = summary {
                    o.extend(s);
                }
                Ok(Value::Object(o))
            }
            "ifc_query" | "ifc_properties" => {
                let id = arg_str(a, "doc").unwrap_or("").to_string();
                match &self.entry(&id, Some("ifc"))?.open {
                    Open::Ifc(d) => {
                        if name == "ifc_query" {
                            d.query(a)
                        } else {
                            d.properties(arg_u64(a, "id", 0))
                        }
                    }
                    _ => Err(format!("{id} is not an IFC model.")),
                }
            }
            "takeoff_open" => {
                let (real, bytes) = self.read_file(arg_str(a, "path").unwrap_or(""))?;
                let name = path_str(&real);
                let json: Value = std::str::from_utf8(&bytes)
                    .ok()
                    .and_then(|t| serde_json::from_str(t).ok())
                    .ok_or_else(|| format!("\"{name}\" is not JSON."))?;
                let r = cc_takeoff::model::read_project(&json)?;
                let t = TakeoffDoc { state: r.state, name, warnings: r.warnings };
                let summary = crate::takeoff::summary(&t);
                let id = self.keep(Open::Takeoff(Box::new(t)), Some(real))?;
                let mut o = Map::new();
                o.insert("doc".into(), json!(id));
                if let Value::Object(s) = summary {
                    o.extend(s);
                }
                Ok(Value::Object(o))
            }
            "takeoff_quantities" | "takeoff_estimate" | "takeoff_export" => {
                let id = arg_str(a, "doc").unwrap_or("").to_string();
                let now = (self.now)();
                let (out_v, bytes) = match &self.entry(&id, Some("takeoff"))?.open {
                    Open::Takeoff(t) => match name {
                        "takeoff_quantities" => (Some(crate::takeoff::quantities(t, arg_str(a, "by").unwrap_or("item"))), None),
                        "takeoff_estimate" => (Some(crate::takeoff::estimate(t)), None),
                        _ => {
                            let ext = ext_of(arg_str(a, "out").unwrap_or(""));
                            if ext != "csv" && ext != "xlsx" {
                                return Err("Takeoffs are exported as .csv or .xlsx.".into());
                            }
                            (None, Some(crate::takeoff::export(t, &ext, now)?))
                        }
                    },
                    _ => return Err(format!("{id} is not a takeoff.")),
                };
                if let Some(v) = out_v {
                    return Ok(v);
                }
                let bytes = bytes.unwrap_or_default();
                let out = self.write(arg_str(a, "out").unwrap_or(""), &bytes, arg_bool(a, "overwrite"), None)?;
                Ok(json!({"out": path_str(&out), "bytes": bytes.len()}))
            }
            "markups_list" => {
                let (real, bytes) = self.read_file(arg_str(a, "path").unwrap_or(""))?;
                crate::markups::list(&bytes, &path_str(&real), a)
            }
            other => Err(format!("Unknown tool: {other}")),
        }
    }
}

/// tools.mjs: lists cut at 2000, with the counts.
fn trim_run(mut r: Value) -> Value {
    let mut counts = Map::new();
    for k in ["added", "changed", "removed"] {
        let n = r.get(k).and_then(Value::as_array).map_or(0, Vec::len);
        counts.insert(k.into(), json!(n));
        if let Some(a) = r.get_mut(k).and_then(Value::as_array_mut) {
            a.truncate(2000);
        }
    }
    if let Some(o) = r.as_object_mut() {
        o.insert("counts".into(), Value::Object(counts));
    }
    r
}

impl Tools for Headless {
    fn tools(&self) -> Vec<ToolDef> {
        headless_defs()
    }
    fn run(&mut self, name: &str, args: Value, _ctx: &CallCtx) -> Result<ToolOut, ToolError> {
        // a tool must never take the server down: a panic becomes the tool's error
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.tool(name, &args)));
        match r {
            Ok(Ok(v)) => Ok(ToolOut::Json(v)),
            Ok(Err(e)) => Err(ToolError::new(e)),
            Err(_) => Err(ToolError::new(format!("{name} failed inside CadCraft (an internal error); the file was not changed."))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::{Dispatcher, Session};

    fn root() -> PathBuf {
        let d = std::env::temp_dir().join(format!("cc-mcp-headless-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("out")).unwrap();
        std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/samples/kitchen-equipment.dwg"), d.join("k.dwg")).unwrap();
        std::fs::canonicalize(d).unwrap()
    }

    fn call(d: &mut Dispatcher, h: &mut Headless, name: &str, args: Value) -> Value {
        let mut s = Session::new("t");
        let r = d
            .handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}}), &mut s, h)
            .unwrap();
        r["result"].clone()
    }

    #[test]
    fn open_edit_save_and_confinement() {
        let r = root();
        let mut h = Headless::new(Roots::new(&[r.display().to_string()], &[], &[]).unwrap());
        let mut d = Dispatcher::new("cadcraft", "CadCraft", "t");
        d.allowed = vec!["read".into(), "edit".into(), "save".into(), "export".into()];
        let open = call(&mut d, &mut h, "drawing_open", json!({"path": "k.dwg"}));
        assert_eq!(open["structuredContent"]["doc"], "d1", "{open}");
        assert_eq!(open["structuredContent"]["format"], "DWG 2018 (AC1032)");
        let run = call(&mut d, &mut h, "command_run", json!({"doc": "d1", "input": ["LINE", "0,0", "@100,0", ""]}));
        assert_eq!(run["structuredContent"]["undo"], "Assistant: LINE");
        assert_eq!(run["structuredContent"]["counts"]["added"], 1);
        let undo = call(&mut d, &mut h, "undo", json!({"doc": "d1"}));
        assert_eq!(undo["structuredContent"]["undone"], json!(["Assistant: LINE"]));
        let saved = call(&mut d, &mut h, "drawing_save", json!({"doc": "d1", "path": "out/k.dxf", "format": "dxf"}));
        assert_eq!(saved["structuredContent"]["format"], "DXF R12");
        assert!(r.join("out/k.dxf").is_file());
        // outside the root, through .., and onto an existing file without overwrite
        let out = call(&mut d, &mut h, "drawing_open", json!({"path": "../../etc/passwd"}));
        assert_eq!(out["isError"], true);
        assert!(out["content"][0]["text"].as_str().unwrap().contains("outside the folders"));
        std::fs::write(r.join("theirs.dxf"), b"x").unwrap();
        let exists = call(&mut d, &mut h, "drawing_convert", json!({"path": "k.dwg", "out": "theirs.dxf"}));
        assert!(exists["content"][0]["text"].as_str().unwrap().contains("already exists"));
        let bad = call(&mut d, &mut h, "drawing_describe", json!({"doc": "d9"}));
        assert_eq!(bad["content"][0]["text"], "No open file d9. Open one first (drawing_open).");
        let _ = std::fs::remove_dir_all(&r);
    }
}
