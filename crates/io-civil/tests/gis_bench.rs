//! Speed of the GIS vector readers/writers on the big files forks/tools/gis-bench.mjs makes once in
//! testfiles/generated/gis/ (the JS side of the same table). Ignored by default:
//!
//!   CC_GIS_BIG=/var/www/CADCRAFT/testfiles/generated/gis cargo test -p cc-io-civil --test gis_bench -- --ignored --nocapture

use std::path::Path;
use std::time::Instant;

fn set(base: &Path) -> cc_io_civil::shapefile::ShpSet {
    let r = |e: &str| std::fs::read(base.with_extension(e)).ok();
    cc_io_civil::shapefile::ShpSet { name: "bench".into(), shp: r("shp"), shx: r("shx"), dbf: r("dbf"), prj: None, cpg: r("cpg") }
}
/// This process's CPU time in ms (user + system, from /proc; 10 ms ticks): the machine is shared, wall time is not fair.
fn cpu_ms() -> f64 {
    let s = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let rest = s.rsplit_once(')').map_or("", |r| r.1);
    let f: Vec<f64> = rest.split_whitespace().map(|x| x.parse().unwrap_or(0.0)).collect();
    (f.get(11).copied().unwrap_or(0.0) + f.get(12).copied().unwrap_or(0.0)) * 10.0
}
/// Median of 3 (after one warm-up): (wall ms, CPU ms, result).
fn time<T>(f: impl Fn() -> T) -> (f64, f64, T) {
    let mut t = Vec::new();
    let mut out = f();
    for _ in 0..3 {
        let (t0, c0) = (Instant::now(), cpu_ms());
        out = f();
        t.push((t0.elapsed().as_secs_f64() * 1000.0, cpu_ms() - c0));
    }
    t.sort_by(|a, b| a.1.total_cmp(&b.1));
    (t[1].0, t[1].1, out)
}

#[test]
#[ignore = "needs the big files (CC_GIS_BIG)"]
fn bench() {
    let Ok(dir) = std::env::var("CC_GIS_BIG") else {
        eprintln!("CC_GIS_BIG not set: skipped");
        return;
    };
    let dir = Path::new(&dir);
    let to_ll = |x: f64, y: f64| (-98.0 + (x - 3_100_000.0) * 3e-6, 30.0 + (y - 10_060_000.0) * 3e-6);
    let pts = set(&dir.join("gis-points-60000.shp"));
    let lns = set(&dir.join("gis-lines-20000x30.shp"));
    let text = std::fs::read_to_string(dir.join("gis-60000pts-20000lns.geojson")).unwrap();
    let (a, a2, lp) = time(|| cc_io_civil::shapefile::read_shapefile(&pts, 10_000_000).unwrap());
    println!("read shapefile points 60000       {a:7.0} ms wall {a2:7.0} ms CPU  ({})", lp.features.len());
    let (b, b2, ll) = time(|| cc_io_civil::shapefile::read_shapefile(&lns, 10_000_000).unwrap());
    println!("read shapefile lines 20000×30     {b:7.0} ms wall {b2:7.0} ms CPU  ({})", ll.features.len());
    let (c, c2, gj) = time(|| cc_io_civil::geojson::read_geojson(&text, "bench", 10_000_000).unwrap());
    println!(
        "read GeoJSON {:3} MB               {c:7.0} ms wall {c2:7.0} ms CPU  ({})",
        text.len() / 1_000_000,
        gj.layers.iter().map(|l| l.features.len()).sum::<usize>()
    );
    let (d, d2, _) = time(|| cc_io_civil::shapefile::write_shapefile("pt", true, &lp.fields, &lp.features, (2026, 10, 17)));
    println!("write shapefile points            {d:7.0} ms wall {d2:7.0} ms CPU  ({})", lp.features.len());
    let g = [cc_io_civil::geojson::WGroup {
        kind: "points".into(),
        geom: "pt".into(),
        has_z: true,
        fields: lp.fields.clone(),
        features: lp.features.clone(),
    }];
    let (e, e2, _) = time(|| cc_io_civil::geojson::write_geojson(&g, &to_ll, &|z| z, Some("b")));
    println!("write GeoJSON points              {e:7.0} ms wall {e2:7.0} ms CPU  ({})", lp.features.len());
}
