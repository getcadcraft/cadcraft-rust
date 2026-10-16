//! CadCraft L4: ModelCraft's file open / save, as the app (modelcraft.js importFile / exports) does it: cc-io-m3d's
//! readers and writers (layer 3) with the SketchUp reader (cc-io-skp) and the DWG reader and writer (cc-io-dwg) joined in
//! — they are layer 3 too, so the joining lives here, shared by the app and `cadcraft-cli m3dio`.

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

use cc_io_m3d::obj::Imported;
use cc_io_m3d::skp::{SkpDef, SkpEdge, SkpFace, SkpInst, SkpLayer, SkpMaterial, SkpModel, SkpTexture};
use cc_io_m3d::{Error, native};
use cc_m3d::Model;
use cc_m3d::json::Out;
use serde_json::{Value, json};

/// An error as the app shows it.
pub fn err_text(e: &Error) -> String {
    e.to_string()
}

/// cc-io-skp's model as cc-io-m3d's SketchUp input.
pub fn skp_model(s: &cc_io_skp::Skp) -> SkpModel {
    SkpModel {
        format_ok: true,
        version: Some(s.version.clone()),
        materials: s
            .materials
            .iter()
            .map(|m| {
                Some(SkpMaterial {
                    name: m.name.clone(),
                    color: m.color.map(|c| c.iter().map(|&x| f64::from(x)).collect()),
                    texture: m.texture.as_ref().map(|t| SkpTexture {
                        name: Some(t.name.clone()),
                        mime: Some(t.mime.to_string()),
                        bytes: Some(t.bytes.clone()),
                    }),
                })
            })
            .collect(),
        layers: s.layers.iter().map(|l| Some(SkpLayer { name: l.name.clone(), visible: l.visible })).collect(),
        defs: s
            .defs
            .iter()
            .map(|d| {
                Some(SkpDef {
                    name: d.name.clone(),
                    kind: d.kind.to_string(),
                    faces: d
                        .faces
                        .iter()
                        .map(|f| SkpFace {
                            loops: f.loops.clone(),
                            front: f.front,
                            layer: f.layer,
                            uv: f.uv.clone(),
                            hidden: f.hidden.unwrap_or(false),
                        })
                        .collect(),
                    edges: d.edges.iter().map(|e| SkpEdge { a: e.a, b: e.b, layer: e.layer, hidden: e.hidden }).collect(),
                    soft: d.soft.clone(),
                    insts: d
                        .insts
                        .iter()
                        .map(|n| SkpInst {
                            def: n.def,
                            m: n.m,
                            name: n.name.clone(),
                            layer: n.layer,
                            hidden: n.hidden,
                            material: n.material,
                        })
                        .collect(),
                })
            })
            .collect(),
        root: Some(s.root),
        notes: s.notes.clone(),
    }
}

/// `dwgVersion(bytes)` (dwg-reader.js).
pub fn dwg_version(bytes: &[u8]) -> String {
    let tag = String::from_utf8_lossy(bytes.get(..6).unwrap_or(bytes)).into_owned();
    let ok = tag.len() == 6 && tag.starts_with("AC") && tag.bytes().skip(2).all(|b| b.is_ascii_digit());
    if !ok {
        return String::new();
    }
    let name = match tag.as_str() {
        "AC1009" => "R12",
        "AC1012" => "R13",
        "AC1014" => "R14",
        "AC1015" => "2000",
        "AC1018" => "2004",
        "AC1021" => "2007",
        "AC1024" => "2010",
        "AC1027" => "2013",
        "AC1032" => "2018",
        _ => "",
    };
    if name.is_empty() { tag } else { format!("{tag} ({name})") }
}

/// `readDWG(bytes)` (dwg-reader.js): cc-dwg's model as JSON, refusing what the app refuses.
pub fn read_dwg_json(bytes: &[u8]) -> Result<Value, String> {
    let tag = String::from_utf8_lossy(bytes.get(..6).unwrap_or(bytes)).into_owned();
    if !(tag.len() == 6 && tag.starts_with("AC") && tag.bytes().skip(2).all(|b| b.is_ascii_digit())) {
        return Err(cc_io_m3d::i18n::t("m3d.dwg.this_is_not_a_dwg_drawing", &[]));
    }
    let m = cc_io_dwg::read_dwg(bytes, &cc_io_dwg::ReadOpts::default());
    let text = cc_io_dwg::V::Obj(m).to_json();
    let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if v.get("blocks").and_then(Value::as_array).is_none_or(Vec::is_empty) {
        let why = v
            .get("report")
            .and_then(|r| r.get("notes"))
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| cc_io_m3d::i18n::t("m3d.dwg.unknown_reason", &[]));
        let ver = dwg_version(bytes);
        let ver = if ver.is_empty() { cc_io_m3d::i18n::t("m3d.dwg.unknown", &[]) } else { ver };
        return Err(cc_io_m3d::i18n::t("m3d.dwg.unreadable", &[("why", &why), ("version", &ver)]));
    }
    Ok(v)
}

/// The app's name for an import: the file name without its extension, at most 80 UTF-16 units, else "Import".
pub fn import_name(file_name: &str) -> String {
    let base = std::path::Path::new(file_name).file_name().and_then(|s| s.to_str()).unwrap_or("");
    let stem = match base.rfind('.') {
        Some(i) => base.get(..i).unwrap_or(""),
        None => base,
    };
    let s = cc_m3d::js::slice16(stem, 80);
    if s.is_empty() { cc_io_m3d::i18n::t("app.import.default", &[]) } else { s }
}

/// The formats ModelCraft imports (by extension).
pub const IMPORT_FORMATS: &[&str] = &["skp", "obj", "glb", "gltf", "dae", "dxf", "dwg"];

/// Where the extension says a file goes: Some(format) when ModelCraft imports it.
pub fn import_format(file_name: &str) -> Option<String> {
    let ext = std::path::Path::new(file_name).extension().and_then(|e| e.to_str())?.to_ascii_lowercase();
    IMPORT_FORMATS.contains(&ext.as_str()).then_some(ext)
}

/// What an import needs, decoded from the file once.
pub enum Input {
    Text(String),
    Bytes(Vec<u8>),
    Json(Value),
    Skp(SkpModel),
}

pub fn decode_input(format: &str, bytes: Vec<u8>) -> Result<Input, String> {
    Ok(match format {
        // Blob.text() / TextDecoder: UTF-8 with replacement characters, a leading byte-order mark dropped
        "obj" | "dae" | "dxf" => {
            Input::Text(String::from_utf8_lossy(bytes.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(&bytes)).into_owned())
        }
        "glb" | "gltf" => Input::Bytes(bytes),
        "dwg" => Input::Json(read_dwg_json(&bytes)?),
        "dwgjson" => Input::Json(serde_json::from_slice(&bytes).map_err(|e| e.to_string())?),
        "skp" => Input::Skp(skp_model(&cc_io_skp::read_skp(&bytes).map_err(|e| e.message)?)),
        "skpjson" => Input::Skp(SkpModel::from_json(&serde_json::from_slice(&bytes).map_err(|e| e.to_string())?)),
        other => return Err(format!("unknown format {other}")),
    })
}

/// The app's import (modelcraft.js importFile): one change of the model, the new group placed at the root.
pub fn run_import(model: &mut Model, format: &str, input: &Input, name: &str, unit: Option<&str>) -> Result<Imported, Error> {
    let label = cc_io_m3d::i18n::t(
        match format {
            "skp" | "skpjson" => "app.undo.import_skp",
            "dae" => "app.undo.import_dae",
            "obj" => "app.undo.import_obj",
            "dxf" | "dwg" | "dwgjson" => "app.undo.import_site",
            _ => "app.undo.import_gltf",
        },
        &[],
    );
    model.change(&label, |m| {
        let x = match (format, input) {
            ("obj", Input::Text(t)) => cc_io_m3d::obj::import_obj(m, t, name)?,
            ("dae", Input::Text(t)) => cc_io_m3d::collada::import_dae(m, t, name)?,
            ("dxf", Input::Text(t)) => cc_io_m3d::site::import_dxf(m, t, name, unit)?,
            (_, Input::Bytes(b)) => cc_io_m3d::gltf::import_gltf(m, b, name)?,
            (_, Input::Json(j)) => cc_io_m3d::site::import_dwg(m, j, name, unit)?,
            (_, Input::Skp(s)) => cc_io_m3d::skp::import_skp(m, s, name)?,
            _ => return Err(Error::Type),
        };
        if let Some(root) = m.root_mut() {
            cc_m3d::model::place(root, &x.def, &cc_m3d::vec::IDENT);
        }
        Ok(x)
    })
}

/// What an export takes besides the model (the defaults are the app's).
#[derive(Clone, Debug, Default)]
pub struct ExportOpts {
    pub name: Option<String>,
    /// ISO date for DAE / IFC headers
    pub date: Option<String>,
    /// OBJ: the MTL file it names
    pub mtl: Option<String>,
    /// DWG: AC1015 … AC1032 (default AC1018)
    pub version: Option<String>,
    pub created_ms: Option<f64>,
    pub guid_seq: bool,
}

/// The formats ModelCraft exports: (format, label, extension).
pub const EXPORT_FORMATS: &[(&str, &str, &str)] = &[
    ("modelcraft", "ModelCraft model", "modelcraft"),
    ("glb", "glTF binary", "glb"),
    ("obj", "OBJ", "obj"),
    ("stl", "STL", "stl"),
    ("dae", "Collada", "dae"),
    ("ifc", "IFC4", "ifc"),
    ("dxf", "DXF", "dxf"),
    ("dwg", "DWG 2004", "dwg"),
];

/// One export: the file's bytes and the report (DXF / DWG notes).
pub fn export(model: &mut Model, format: &str, o: &ExportOpts) -> Result<(Vec<u8>, Value), String> {
    let name = o.name.as_deref().unwrap_or("ModelCraft model");
    let date = o.date.as_deref().unwrap_or("2026-10-10T12:00:00.000Z");
    Ok(match format {
        "glb" => (cc_io_m3d::gltf::export_glb(model), json!({})),
        "obj" => (cc_io_m3d::obj::export_obj(model, o.mtl.as_deref()).into_bytes(), json!({})),
        "mtl" => (cc_io_m3d::obj::export_mtl(model).into_bytes(), json!({})),
        "stl" => (cc_io_m3d::stl::export_stl(model), json!({})),
        "dae" => (cc_io_m3d::collada::export_dae(model, name, date).into_bytes(), json!({})),
        "ifc" => (cc_io_m3d::ifc::export_ifc(model, name, date).into_bytes(), json!({})),
        "modelcraft" => (native::export_native(model).into_bytes(), json!({})),
        "dxf" => {
            let (text, report) = cc_io_m3d::cad::export_dxf(model).map_err(|e| err_text(&e))?;
            (text.into_bytes(), json!({ "report": report.to_value() }))
        }
        "dwgmodel" => {
            let (dwg, report) = cc_io_m3d::cad::to_dwg_model(model).map_err(|e| err_text(&e))?;
            let mut o = Out::new(true);
            o.value(&dwg);
            (o.s.into_bytes(), json!({ "report": report.to_value() }))
        }
        "dwgmodel-raw" => {
            // dev: serde_json's own text, which keeps −0 (the parity text writes it as 0, as JSON.stringify does)
            let (dwg, report) = cc_io_m3d::cad::to_dwg_model(model).map_err(|e| err_text(&e))?;
            (serde_json::to_string(&dwg).unwrap_or_default().into_bytes(), json!({ "report": report.to_value() }))
        }
        "dwg" => {
            let (dwg, report) = cc_io_m3d::cad::to_dwg_model(model).map_err(|e| err_text(&e))?;
            let v = match cc_io_dwg::w_js::from_json(&dwg, 0) {
                cc_io_dwg::V::Obj(o) => o,
                _ => return Err("the DWG model is not an object".into()),
            };
            let opts = cc_io_dwg::WriteOpts {
                version: o.version.clone().unwrap_or_else(|| "AC1018".into()),
                created_ms: o.created_ms,
                guid: if o.guid_seq { cc_io_dwg::GuidMode::Sequence } else { cc_io_dwg::GuidMode::Random },
            };
            let (bytes, _wrep) = cc_io_dwg::write_dwg_opts(&v, &opts)?;
            (bytes, json!({ "report": report.to_value() }))
        }
        other => return Err(format!("unknown format {other}")),
    })
}
