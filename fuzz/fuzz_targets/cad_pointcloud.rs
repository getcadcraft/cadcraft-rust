//! Fuzz CadCraft's point cloud under the drawing (M4.50: pointcloud.js over CivilCraft's LAS / LAZ reader, XYZ / PTS
//! text read here, then cloud-cad.js's colours, slice, snap grid and draw): any bytes give a cloud or an error, never
//! a panic, hang or OOM. The first byte picks the file name's extension (.las .laz .xyz .pts) and the drawing units.
#![no_main]

use cc_engine::cloud::CloudAttach;
use cc_geom::P2;
use cc_io_civil::pointcloud as pcl;
use cc_io_civil::raster::InFile;
use libfuzzer_sys::fuzz_target;

/// As civil_las: laz-rs panics on damaged data are caught by cc-io-civil's guard; any other panic aborts.
fn guard() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if info.location().is_some_and(|l| l.file().contains("/laz-")) {
                return;
            }
            prev(info);
        }));
    });
}

fuzz_target!(|data: &[u8]| {
    guard();
    let Some((&sel, rest)) = data.split_first() else { return };
    let ext = ["las", "laz", "xyz", "pts"][usize::from(sel & 3)];
    let units = ["", "meters", "feet", "millimeters"][usize::from((sel >> 2) & 3)];
    let tr = cc_cad::i18n::Catalog::default();
    let f = InFile { name: format!("f.{ext}"), bytes: rest.to_vec() };
    let Ok(info) = pcl::read_cloud_info(&tr, &f) else { return };
    let Ok(cloud) = pcl::load_cloud(&tr, &f, &info, 1000.0) else { return };
    for m in ["rgb", "height", "intensity", "class", "single"] {
        std::hint::black_box(pcl::colorize(&cloud, m, [1, 2, 3]));
    }
    std::hint::black_box(pcl::cloud_summary(&tr, &cloud));
    let mut pc = CloudAttach::default();
    pc.attach(cloud, if sel & 16 != 0 { Some(P2::new(10.0, -5.0)) } else { None });
    for on in [false, true] {
        pc.slice.on = on;
        pc.changed();
        if let Some(b) = pc.bbox(units) {
            let c = P2::new((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
            std::hint::black_box(pc.snap(c, (b[2] - b[0]).abs() / 50.0 + 1e-6, units));
            let w = (b[2] - b[0]).max(b[3] - b[1]).max(1e-9);
            let k = 64.0 / w;
            std::hint::black_box(pc.draw([k, 0.0, 0.0, -k, -k * b[0], k * b[3]], 64.0, 48.0, 1.5, units).map(|r| r.px.len()));
        }
    }
    std::hint::black_box(pc.matrix3d(0.001, units));
});
