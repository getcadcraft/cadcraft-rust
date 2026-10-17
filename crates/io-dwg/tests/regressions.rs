//! Regression files found by fuzzing (each made the reader misbehave before its fix).

/// This thread's CPU time in ms (Linux: /proc/thread-self/stat utime + stime, 100 ticks a second), so a busy machine
/// does not fail the bound; elsewhere None and the caller falls back to wall time.
fn thread_cpu_ms() -> Option<f64> {
    let s = std::fs::read_to_string("/proc/thread-self/stat").ok()?;
    // fields after the command name (which may hold spaces) start at state; utime and stime are fields 14 and 15
    let rest = s.rsplit_once(')')?.1;
    let f: Vec<&str> = rest.split_whitespace().collect();
    let ticks = f.get(11)?.parse::<f64>().ok()? + f.get(12)?.parse::<f64>().ok()?;
    Some(ticks * 10.0)
}

/// How long `f` took: CPU time where the system tells it, wall time otherwise.
fn cost_ms<T>(f: impl FnOnce() -> T) -> (T, f64) {
    let (c0, t0) = (thread_cpu_ms(), std::time::Instant::now());
    let v = f();
    let ms = match (c0, thread_cpu_ms()) {
        (Some(a), Some(b)) => b - a,
        _ => t0.elapsed().as_secs_f64() * 1000.0,
    };
    (v, ms)
}

/// A negative EED size: the JS reader stops reading EED there; the port re-read the object 10 000 times (0.4 s).
#[test]
fn negative_eed_size_stops_the_eed_loop() {
    let bytes = include_bytes!("data/fuzz-negative-eed-size.dwg");
    let (model, ms) = cost_ms(|| cc_io_dwg::read_dwg(bytes, &cc_io_dwg::ReadOpts::default()));
    let notes = model.obj("report").and_then(|r| r.arr("notes")).cloned().unwrap_or_default();
    assert!(notes.iter().any(|n| n.as_str() == Some("Object map entry 1 points at object 231599106C9D1A000000000.")), "{notes:?}");
    assert!(ms < 200.0, "took {ms} ms of CPU");
}
