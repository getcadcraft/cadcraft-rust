//! plot.js port (M4.12): the 0.5 `bin/test-cadcraft.mjs` "Plot to PDF" checks that need no PDF reader, plus the
//! file's own structure (every xref offset lands on its object; the deflated content inflates to the plain one).
//! Byte parity with the frozen JS is `forks/tools/plotdiff.mjs`.

use cc_cad::Doc;
use cc_io_cad::plot::{PlotOpts, display_list, extents_of, model_sheet, parse_scale, plot_pdf, scale_label, to_pdf, win_ansi};
use serde_json::json;

fn doc() -> Doc {
    let mut d = Doc::new();
    d.load(&json!({
        "meta": { "name": "Plan", "units": "millimeters" },
        "layers": [{ "name": "0" }, { "name": "NOPLOT", "plot": false }],
        "entities": [
            { "id": 1, "type": "line", "layer": "0", "x1": 0, "y1": 0, "x2": 1000, "y2": 0 },
            { "id": 2, "type": "circle", "layer": "0", "cx": 500, "cy": 300, "r": 200, "color": 1 },
            { "id": 3, "type": "text", "layer": "0", "x": 0, "y": 600, "h": 50, "s": "Room → 1" },
            { "id": 4, "type": "line", "layer": "NOPLOT", "x1": 0, "y1": 0, "x2": 0, "y2": 5000 },
            { "id": 5, "type": "hatch", "layer": "0", "solid": true, "loops": [[0, 0, 100, 0, 100, 100]] }
        ]
    }));
    d
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}
fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).rposition(|w| w == needle)
}
fn line_at(b: &[u8], at: usize) -> &str {
    let rest = &b[at..];
    let end = rest.iter().position(|c| *c == b'\n').unwrap_or(rest.len());
    std::str::from_utf8(&rest[..end]).unwrap_or("")
}

/// Every xref offset lands on its `N 0 obj` (byte offsets, on bytes: the binary comment line is not UTF-8).
fn offsets_ok(pdf: &[u8]) -> bool {
    let Some(x) = rfind(pdf, b"startxref\n") else { return false };
    let at: usize = line_at(pdf, x + 10).trim().parse().unwrap();
    assert_eq!(line_at(pdf, at), "xref");
    let head = line_at(pdf, at + 5);
    let n: usize = head.split(' ').nth(1).unwrap().parse().unwrap();
    let mut pos = at + 5 + head.len() + 1 + 20; // past the free entry (20 bytes)
    for k in 1..n {
        let off: usize = line_at(pdf, pos)[..10].parse().unwrap();
        if !pdf[off..].starts_with(format!("{k} 0 obj").as_bytes()) {
            return false;
        }
        pos += 20;
    }
    true
}

#[test]
fn model_plot_fitted_and_at_a_scale() {
    let d = doc();
    let r = plot_pdf(&d, &PlotOpts { date_ms: 1_792_000_000_000.0, ..PlotOpts::default() }).unwrap();
    assert!(r.pdf.bytes.starts_with(b"%PDF-1.4\n") && r.pdf.bytes.ends_with(b"%%EOF\n"));
    assert!(offsets_ok(&r.pdf.bytes), "every xref offset lands on its object");
    assert!(r.scale.as_deref().unwrap().ends_with("(fitted)") && r.fits);
    assert_eq!(r.pdf.missing, 1, "the arrow prints as ?");
    let s = plot_pdf(&d, &PlotOpts { scale: Some("1:50".into()), paper: "iso-a4".into(), ..PlotOpts::default() }).unwrap();
    assert_eq!(s.scale.as_deref(), Some("1:50"));
    let big = plot_pdf(&d, &PlotOpts { scale: Some("1:1".into()), paper: "iso-a4".into(), ..PlotOpts::default() }).unwrap();
    assert!(!big.fits, "1:1 of a metre-wide plan does not fit on A4");
    assert!(plot_pdf(&d, &PlotOpts { scale: Some("nonsense".into()), ..PlotOpts::default() }).is_err());
    assert!(plot_pdf(&d, &PlotOpts { layout: Some("Nope".into()), ..PlotOpts::default() }).is_err());
    assert!(plot_pdf(&Doc::new(), &PlotOpts::default()).is_err(), "nothing to plot");
}

#[test]
fn no_plot_layers_are_left_out_and_the_deflated_stream_is_the_plain_one() {
    let d = doc();
    let ext = extents_of(&d).unwrap();
    assert!(ext.max_y < 5000.0, "the NOPLOT line is not in the extents");
    let sheet = model_sheet(&d, &ext, "iso-a3", false, None);
    let list = display_list(&d, &sheet, false);
    let plain = to_pdf(&list, 0.25, "Plan", 0.0, false);
    let packed = to_pdf(&list, 0.25, "Plan", 0.0, true);
    let stream = |b: &[u8]| -> Vec<u8> {
        let i = find(b, b"stream\n").unwrap() + 7;
        let j = rfind(b, b"\nendstream").unwrap();
        b[i..j].to_vec()
    };
    let inflated = miniz_oxide::inflate::decompress_to_vec_zlib(&stream(&packed.bytes)).unwrap();
    assert_eq!(inflated, stream(&plain.bytes));
    let text = String::from_utf8(inflated).unwrap();
    assert!(text.contains(" rg") && text.contains("BT /F1 1 Tf"), "a colour fill and Helvetica text");
}

#[test]
fn scales_and_text_encoding() {
    let mut d = doc();
    assert_eq!(parse_scale(&d, Some("fit")), Ok(None));
    assert_eq!(parse_scale(&d, Some("1:100")), Ok(Some(100.0)));
    assert_eq!(parse_scale(&d, Some("2:1")), Ok(Some(0.5)));
    d.meta.insert("units".into(), json!("inches"));
    assert_eq!(parse_scale(&d, Some("1/4\" = 1'-0\"")), Ok(Some(48.0)));
    assert_eq!(scale_label(&d, "mm", cc_io_cad::plot::s_of(&d, "mm", 48.0)), "1/4\" = 1'-0\"");
    let (s, w, miss) = win_ansi("a(b)\\ é ✓");
    assert_eq!(s, "a\\(b\\)\\\\ \\351 ?");
    assert!(w > 0.0 && miss == 1);
}

#[test]
fn hostile_objects_do_not_panic() {
    let mut d = Doc::new();
    d.load(&json!({ "entities": [
        { "id": 1, "type": "line", "layer": "0", "x1": 0, "y1": 0, "x2": 1e308, "y2": -1e308 },
        { "id": 2, "type": "text", "layer": "0", "x": 0, "y": 0, "h": 1e-300, "s": "\u{0}\u{1F600}" },
        { "id": 3, "type": "arc", "layer": "0", "cx": 0, "cy": 0, "r": 5, "a0": 0, "a1": 1e9 }
    ]}));
    let _ = plot_pdf(&d, &PlotOpts::default());
    let _ = plot_pdf(
        &d,
        &PlotOpts {
            window: Some(cc_cad_geom::geom::Bounds { min_x: f64::NAN, min_y: 0.0, max_x: 1.0, max_y: 1.0 }),
            ..PlotOpts::default()
        },
    );
}

/// pictures.js `pdfImage` with a dpi cap: the size, the pixels (FNV-1a of the inflated RGB and alpha) and the
/// shrunk / kept flags as the frozen 0.5 JS gives them (tests/data/pics, made with Pillow for these tests).
#[test]
fn pdf_image_matches_the_js() {
    use cc_cad::pictures::PicRec;
    use cc_io_cad::pdfimage::pdf_image;
    let fnv = |b: &[u8]| b.iter().fold(0x811c_9dc5u32, |h, x| (h ^ u32::from(*x)).wrapping_mul(0x0100_0193));
    let file = |n: &str| std::fs::read(format!("{}/tests/data/pics/{n}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    // (file, mm wide, mm high, dpi, w, h, rgb hash, alpha hash (0 = none), shrunk, kept): node dpicheck.mjs
    type Case = (&'static str, f64, f64, f64, u32, u32, u32, u32, bool, bool);
    let cases: &[Case] = &[
        ("rgba.png", 5.0, 4.0, 150.0, 30, 24, 0x8ea3d8ad, 0x83d82d15, true, false),
        ("rgba.png", 3.3, 2.1, 300.0, 39, 25, 0xa5cac187, 0xf1bda121, true, false),
        ("pal.png", 1.0, 1.0, 150.0, 6, 6, 0x17c897a9, 0xf31fc30d, true, false),
        ("gray16.png", 1.7, 1.2, 150.0, 10, 7, 0x9f907957, 0x0, true, false),
        ("photo.jpg", 2.0, 2.0, 300.0, 64, 48, 0x4fcdf684, 0x0, false, true),
        ("photo.jpg", 20.0, 20.0, 300.0, 64, 48, 0x4fcdf684, 0x0, false, false),
        ("rgba.png", 100.0, 100.0, 150.0, 40, 30, 0x8e3cf73d, 0x8c2caced, false, false),
    ];
    for &(n, mw, mh, dpi, w, h, rgb, a, shrunk, kept) in cases {
        let bytes = file(n);
        let (mime, pw, ph) = cc_cad::pictures::sniff_picture(&bytes).unwrap();
        let rec = PicRec { name: n.into(), bytes: bytes.into(), mime, w: pw, h: ph };
        let im = pdf_image(&rec, dpi, mw, mh).unwrap();
        let px = if im.filter == "FlateDecode" { miniz_oxide::inflate::decompress_to_vec_zlib(&im.data).unwrap() } else { im.data.clone() };
        let al = im.smask.as_ref().map_or(0, |s| fnv(&miniz_oxide::inflate::decompress_to_vec_zlib(s).unwrap()));
        assert_eq!((im.w, im.h, fnv(&px), al, im.shrunk, im.kept), (w, h, rgb, a, shrunk, kept), "{n} {mw}×{mh} mm at {dpi} dpi");
    }
}

/// plot.js `num`: the fast `{:.4}` path gives what `toFixed(4)` gives (the exact path), halfway values included.
#[test]
fn num_matches_to_fixed() {
    let slow = |v: f64| -> String {
        if !v.is_finite() {
            return "0".into();
        }
        let s = cc_cad::js::to_fixed_exact(if v.abs() < 5e-5 { 0.0 } else { v }, 4);
        if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
    };
    let mut vals = vec![
        0.0,
        -0.0,
        5e-5,
        -5e-5,
        4.9999e-5,
        0.03125,
        -0.03125,
        1.00005,
        2.5e-4,
        0.15625,
        123.45675,
        1e15,
        -3e17,
        4.5e11,
        1e21,
        f64::NAN,
    ];
    let mut x = 0x1234_5678_9abc_def0u64;
    for _ in 0..200_000 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let m = (x >> 11) as f64 / (1u64 << 53) as f64;
        let e = ((x & 0x3f) as i32) - 24;
        let v = (m - 0.5) * 2f64.powi(e);
        vals.push(v);
        vals.push((v * 32.0).round() / 32.0);
        vals.push((v * 20000.0).round() / 20000.0);
    }
    for v in vals {
        assert_eq!(cc_io_cad::plot::num(v), slow(v), "{v:e}");
    }
}
