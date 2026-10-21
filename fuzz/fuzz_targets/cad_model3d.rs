//! Fuzz the 3D model a drawing file brings and what CadCraft makes of it (model3d.js `fromFile`, `forFile3d`,
//! `outline2d` and the solids, M4.24): any doc.load input — CadCraft's own model JSON (`model3d`), loose 3D faces
//! (`faces3d`), any 2D objects — reads, writes back as 3DFACEs + JSON, and extrudes / revolves / sweeps its outlines
//! without panicking, hanging or running out of memory.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() > 1 << 16 {
        return;
    }
    let Ok(input) = serde_json::from_slice::<serde_json::Value>(data) else { return };
    let mut doc = cc_cad::Doc::new();
    doc.load(&input);
    let mut m = cc_io_cad::m3dmodel::model_of(&doc, "Faces");
    let _ = cc_io_cad::m3dmodel::for_file3d(&mut m, "", doc.view3d.as_ref());
    let t = |k: &str| k.to_string();
    let tv = |k: &str, _: &[(&str, String)]| k.to_string();
    let ents: Vec<cc_cad::entity::Entity> = doc.model().take(8).cloned().collect();
    let mut path: Option<Vec<[f64; 2]>> = None;
    for e in &ents {
        let Ok(ring) = cc_io_cad::m3dmodel::outline2d(e) else { continue };
        let _ = cc_io_cad::m3dmodel::make_prism(&mut m, &ring, 0.0, 1.0, "Extrusion", &t);
        let _ = cc_io_cad::m3dmodel::make_tapered(&mut m, &ring, 0.0, 1.0, 0.2, &tv);
        let _ = cc_io_cad::m3dmodel::make_revolved(&mut m, &ring, [0.0, -1.0], [1.0, -1.0], 1.0, &tv);
        if let Some(p) = &path {
            let _ = cc_io_cad::m3dmodel::make_swept(&mut m, &ring, p, false, &tv);
        }
        path = Some(ring);
    }
    let _ = cc_io_cad::m3dmodel::solid_volumes(&m);
    let _ = cc_io_cad::m3dmodel::for_file3d(&mut m, "meters", None);
});
