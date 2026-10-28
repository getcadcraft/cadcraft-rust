//! pointcloud.js (M4.50): LAS records read raw (intensity, class, colour), the keep budget, colours, slices and the
//! snap grid, on LAS files made here (LAS 1.2, formats 0 and 2) — the corpus files are compared with the JS by
//! forks/tools/pcbench-cadcloud.mjs and kdiff-cadcloud.mjs.

use cc_cad::i18n::Catalog;
use cc_io_civil::las;
use cc_io_civil::pointcloud as pcl;
use cc_io_civil::raster::InFile;

/// A LAS 1.2 file: `fmt` 0 (20-byte records) or 2 (26: + r g b), scale 0.001, offset (500000, 4000000, 0), the points
/// given as (x, y, z, intensity, class, rgb16).
/// A point: x, y, z, intensity, class, 16-bit colour.
type Pt = (f64, f64, f64, u16, u8, [u16; 3]);

fn las(fmt: u8, pts: &[Pt]) -> Vec<u8> {
    let len: u16 = if fmt == 2 { 26 } else { 20 };
    let mut b = vec![0u8; 227];
    b[..4].copy_from_slice(b"LASF");
    b[24] = 1;
    b[25] = 2;
    b[94..96].copy_from_slice(&227u16.to_le_bytes());
    b[96..100].copy_from_slice(&227u32.to_le_bytes());
    b[104] = fmt;
    b[105..107].copy_from_slice(&len.to_le_bytes());
    b[107..111].copy_from_slice(&(pts.len() as u32).to_le_bytes());
    for (i, s) in [0.001f64, 0.001, 0.001].iter().enumerate() {
        b[131 + i * 8..139 + i * 8].copy_from_slice(&s.to_le_bytes());
    }
    for (i, o) in [500000.0f64, 4000000.0, 0.0].iter().enumerate() {
        b[155 + i * 8..163 + i * 8].copy_from_slice(&o.to_le_bytes());
    }
    for &(x, y, z, it, cls, rgb) in pts {
        let mut r = vec![0u8; usize::from(len)];
        for (k, (v, o)) in [(x, 500000.0), (y, 4000000.0), (z, 0.0)].iter().enumerate() {
            let q = ((v - o) / 0.001).round() as i32;
            r[k * 4..k * 4 + 4].copy_from_slice(&q.to_le_bytes());
        }
        r[12..14].copy_from_slice(&it.to_le_bytes());
        r[15] = cls & 31;
        if fmt == 2 {
            for (k, c) in rgb.iter().enumerate() {
                r[20 + k * 2..22 + k * 2].copy_from_slice(&c.to_le_bytes());
            }
        }
        b.extend_from_slice(&r);
    }
    b
}

fn cat() -> Catalog {
    Catalog::new(&serde_json::json!({ "pc.read_short": "Read {read} of the {count} points the header lists." }))
}

#[test]
fn raw_records_carry_intensity_class_and_colour() {
    let bytes = las(2, &[(500001.0, 4000002.0, 3.5, 77, 6, [65535, 0, 256]), (500002.5, 4000001.25, 0.25, 9, 2, [0, 512, 0])]);
    let f = InFile { name: "t.las".into(), bytes };
    let h = las::read_las_header_lenient(&f).unwrap_or_else(|e| panic!("{e}"));
    let mut recs = Vec::new();
    let mut notes = Vec::new();
    let n = las::scan_records(&f, &h, &mut notes, &mut |r| {
        recs.push(r.to_vec());
        Ok(())
    })
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(n, 2.0);
    assert_eq!(recs.len(), 2);
    assert!(recs.iter().all(|r| r.len() == 26));
    let info = pcl::read_cloud_info(&cat(), &f).unwrap_or_else(|e| panic!("{e}"));
    assert!(info.color && info.intensity && info.classes);
    let c = pcl::load_cloud(&cat(), &f, &info, 2e6).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(c.n, 2);
    assert_eq!(c.origin, [500001.0, 4000001.0, 0.0]);
    // survey coordinates keep their millimetres: offsets from the origin
    assert_eq!(pcl::point_at(&c, 1), [500002.5, 4000001.25, 0.25]);
    assert_eq!(c.inten.as_deref(), Some(&[77u16, 9][..]));
    assert_eq!(c.cls.as_deref(), Some(&[6u8, 2][..]));
    // 16-bit colours (a value over 255): the high byte
    assert_eq!(c.rgb.as_deref(), Some(&[255u8, 0, 1, 0, 2, 0][..]));
    assert_eq!(pcl::color_modes(&c), vec!["rgb", "height", "intensity", "class", "single"]);
    assert_eq!(pcl::colorize(&c, "class", [0, 0, 0]), vec![215, 70, 50, 160, 120, 70]);
    let s = pcl::cloud_summary(&cat(), &c);
    assert_eq!(s.classes.iter().map(|x| (x.0, x.2)).collect::<Vec<_>>(), vec![(2, 1.0), (6, 1.0)]);
}

#[test]
fn the_budget_keeps_points_evenly_and_a_short_file_says_so() {
    let pts: Vec<_> = (0..5000).map(|i| (500000.0 + f64::from(i) * 0.01, 4000000.0, f64::from(i % 7), 0, 1, [0, 0, 0])).collect();
    let mut bytes = las(0, &pts);
    let f = InFile { name: "big.las".into(), bytes: bytes.clone() };
    let info = pcl::read_cloud_info(&cat(), &f).unwrap_or_else(|e| panic!("{e}"));
    let c = pcl::load_cloud(&cat(), &f, &info, 1000.0).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(c.read, 5000.0);
    assert!(c.n <= 1000 && c.n >= 500, "{}", c.n);
    assert_eq!(c.rgb, None);
    // every point read counts for the box, kept or not
    assert_eq!(c.max[0], 500049.99);
    // a cut file: the whole records it has
    bytes.truncate(227 + 20 * 10 + 7);
    let f = InFile { name: "cut.las".into(), bytes };
    let info = pcl::read_cloud_info(&cat(), &f).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(info.count, 10.0);
    let c = pcl::load_cloud(&cat(), &f, &info, 1000.0).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(c.n, 10);
}

#[test]
fn slices_and_the_snap_grid() {
    let pts: Vec<_> =
        (0..400).map(|i| (500000.0 + f64::from(i % 20), 4000000.0 + f64::from(i / 20), f64::from(i % 5) * 0.5, 0, 1, [0, 0, 0])).collect();
    let f = InFile { name: "g.las".into(), bytes: las(0, &pts) };
    let info = pcl::read_cloud_info(&cat(), &f).unwrap_or_else(|e| panic!("{e}"));
    let c = pcl::load_cloud(&cat(), &f, &info, 2e6).unwrap_or_else(|e| panic!("{e}"));
    let s = pcl::slice_of(&c, 2, 1.0, 0.1);
    assert_eq!(s.len(), 80);
    let g = pcl::PlanGrid::new(&c, Some(&s), 0.0);
    // in the slice: z = 1 (i % 5 = 2), e.g. the point at (2, 4); (3, 4) is at z = 1.5, outside it
    let i = g.nearest(&c, 500002.9, 4000004.05, 0.95).unwrap_or(usize::MAX);
    assert_eq!(pcl::point_at(&c, i)[..2], [500002.0, 4000004.0]);
    assert_eq!(g.nearest(&c, 500003.5, 4000004.5, 0.1), None);
    // a radius far bigger than the cloud's cells: nothing (the JS's 40 000-cell limit)
    assert_eq!(g.nearest(&c, 500003.0, 4000004.0, 1e6), None);
    let all = pcl::PlanGrid::new(&c, None, 0.0);
    assert_eq!(all.size, 400);
}
