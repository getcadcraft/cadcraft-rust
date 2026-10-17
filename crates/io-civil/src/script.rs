//! The file ops of the call-script runner (`cadcraft-cli civil`); the core ops are cc-civil's
//! ([`cc_civil::script`]). Each module adds its arm here.

use cc_civil::script::{Call, Registry};
use serde_json::Value;

/// This crate's ops (Some when the op is one of ours).
pub fn handle(reg: &mut Registry, c: &Call) -> Option<Result<Value, String>> {
    let (module, _) = c.op.split_once('.').unwrap_or((c.op, ""));
    // one arm per module, e.g. "crs" => crate::crs_ops::handle(reg, c),
    match module {
        "lx" => crate::civil_ops::handle(reg, c),
        "scene" | "dxf" | "dwg" | "plot" | "font" | "sheets" | "xml" => crate::civil_ops::handle2(reg, c),
        "gis" | "dbf" | "shp" | "geojson" | "kml" | "zip" => crate::gis_ops::handle(reg, c),
        "cad" => crate::gis_ops::cad_handle(reg, c),
        "map" => crate::gis_ops::map_handle(reg, c),
        "gpkg" => crate::gis_ops::gpkg_handle(reg, c),
        "gread" | "gexp" | "glayers" => crate::gis_ops::gisx_handle(reg, c),
        "crs" | "frame" | "pick" | "proj" => crate::crs_ops::handle(reg, c),
        "terrain" | "thin" | "raster" | "las" | "gtw" => crate::terrain_ops::handle(reg, c),
        "glb" | "obj" => {
            let name = c.arg(0).and_then(Value::as_str).unwrap_or("");
            let Some(sc) = reg.get::<cc_civil::site3d::Scene>(name) else { return Some(Err(format!("no object {name}"))) };
            let nm = c.arg(1).and_then(|o| o.get("name")).map_or_else(|| "Site".to_string(), cc_civil::js::to_js_string);
            Some(Ok(if c.op == "glb.export" {
                serde_json::json!({ "$base64": crate::civil_ops::b64(&crate::site3d_export::export_glb(sc, &nm)) })
            } else {
                Value::String(crate::site3d_export::export_obj(sc, &nm))
            }))
        }
        _ => None,
    }
}
