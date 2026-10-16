//! Fuzz ModelCraft's file readers (cc-io-m3d): the first byte picks the reader, the rest is the file. Every reader
//! must import or refuse, never panic, hang or run out of memory; what it imports is then written back out through
//! every exporter (OBJ, MTL, GLB, STL, Collada, IFC, DXF, the DWG model, the native file) and through the 3D-print
//! check, so the writers see hostile models too.
//!
//! Format byte (mod 9): 0 OBJ, 1 glTF/GLB, 2 Collada, 3 DXF site plan, 4 DWG model JSON, 5 SketchUp model JSON,
//! 6 native .modelcraft JSON, 7 XML parser alone, 8 print3d on raw f32 triangles.
#![no_main]

use cc_io_m3d::print3d::{CheckOpts, Tri, mesh_from_triangles, print_check, stl_binary, three_mf, weld};
use cc_m3d::Model;
use libfuzzer_sys::fuzz_target;

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(b)).into_owned()
}

fn write_all(model: &mut Model) {
    let _ = cc_io_m3d::obj::export_obj(model, Some("x.mtl"));
    let _ = cc_io_m3d::obj::export_mtl(model);
    let _ = cc_io_m3d::gltf::export_glb(model);
    let _ = cc_io_m3d::stl::export_stl(model);
    let _ = cc_io_m3d::collada::export_dae(model, "x", "2026-10-11T12:00:00.000Z");
    let _ = cc_io_m3d::ifc::export_ifc(model, "x", "2026-10-11T12:00:00.000Z");
    let _ = cc_io_m3d::cad::export_dxf(model);
    let _ = cc_io_m3d::cad::to_dwg_model(model);
    let (tris, _) = cc_io_m3d::printmc::print_triangles(model, &[], &[]);
    let _ = print_check(&mesh_from_triangles(&tris), &CheckOpts::default());
    let s = cc_io_m3d::native::export_native(model);
    if let Ok(j) = serde_json::from_str::<serde_json::Value>(&s) {
        let _ = cc_io_m3d::native::load_native(&j);
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&fmt, body)) = data.split_first() else { return };
    let mut m = Model::new();
    let r = match fmt % 9 {
        0 => m.change("x", |mm| cc_io_m3d::obj::import_obj(mm, &text(body), "x")),
        1 => m.change("x", |mm| cc_io_m3d::gltf::import_gltf(mm, body, "x")),
        2 => m.change("x", |mm| cc_io_m3d::collada::import_dae(mm, &text(body), "x")),
        3 => m.change("x", |mm| cc_io_m3d::site::import_dxf(mm, &text(body), "x", None)),
        4 => {
            let Ok(j) = serde_json::from_slice::<serde_json::Value>(body) else { return };
            m.change("x", |mm| cc_io_m3d::site::import_dwg(mm, &j, "x", Some("mm")))
        }
        5 => {
            let Ok(j) = serde_json::from_slice::<serde_json::Value>(body) else { return };
            let skp = cc_io_m3d::skp::SkpModel::from_json(&j);
            m.change("x", |mm| cc_io_m3d::skp::import_skp(mm, &skp, "x"))
        }
        6 => {
            let Ok(j) = serde_json::from_slice::<serde_json::Value>(body) else { return };
            if let Ok(mut loaded) = cc_io_m3d::native::load_native(&j) {
                write_all(&mut loaded);
            }
            return;
        }
        7 => {
            let _ = cc_io_m3d::xml::parse_xml(&text(body), 100_000);
            return;
        }
        _ => {
            let f: Vec<f64> = body.chunks_exact(4).map(|c| f64::from(f32::from_le_bytes([c[0], c[1], c[2], c[3]]))).collect();
            let tris: Vec<Tri> = f
                .chunks_exact(9)
                .map(|c| Tri { p: [[c[0], c[1], c[2]], [c[3], c[4], c[5]], [c[6], c[7], c[8]]], color: None })
                .collect();
            let mesh = mesh_from_triangles(&tris);
            let _ = print_check(&mesh, &CheckOpts::default());
            let w = weld(&mesh, 1e-4);
            let _ = stl_binary(&w, "x");
            let _ = three_mf(&w, "x", "x");
            return;
        }
    };
    if let Ok(x) = r {
        if let Some(root) = m.root_mut() {
            cc_m3d::model::place(root, &x.def, &cc_m3d::vec::IDENT);
        }
        write_all(&mut m);
    }
});
