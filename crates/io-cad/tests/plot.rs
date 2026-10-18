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
