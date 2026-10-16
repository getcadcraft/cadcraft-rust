//! Regression files found by fuzzing (each made the reader misbehave before its fix).

use std::time::Instant;

/// A negative EED size: the JS reader stops reading EED there; the port re-read the object 10 000 times (0.4 s).
#[test]
fn negative_eed_size_stops_the_eed_loop() {
    let bytes = include_bytes!("data/fuzz-negative-eed-size.dwg");
    let t = Instant::now();
    let model = cc_io_dwg::read_dwg(bytes, &cc_io_dwg::ReadOpts::default());
    let ms = t.elapsed().as_millis();
    let notes = model.obj("report").and_then(|r| r.arr("notes")).cloned().unwrap_or_default();
    assert!(notes.iter().any(|n| n.as_str() == Some("Object map entry 1 points at object 231599106C9D1A000000000.")), "{notes:?}");
    assert!(ms < 200, "took {ms} ms");
}
