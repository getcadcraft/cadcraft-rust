//! The fork-parity contract for the IFC engines: `parity ifc-step|ifc-geom <file>` and `bench ifc-step|ifc-geom <file>`.

use std::io::Write as _;
use std::process::ExitCode;
use std::time::Instant;

fn json_str(s: &str) -> String {
    let mut o = String::new();
    cc_io_ifc::util::push_json_str(&mut o, s);
    o
}

fn print_error(code: &str, message: &str) {
    println!(r#"{{"$error":{{"code":{},"message":{}}}}}"#, json_str(code), json_str(message));
}

fn read(path: &str) -> Result<Vec<u8>, ExitCode> {
    std::fs::read(path).map_err(|e| {
        print_error("io", &e.to_string());
        ExitCode::from(1)
    })
}

/// `parity <engine> <file>` for the IFC engines; None when the engine is not one of them.
pub fn parity(engine: &str, file: &str) -> Option<ExitCode> {
    match engine {
        "ifc-step" => Some(match read(file) {
            Err(c) => c,
            Ok(bytes) => {
                let out = std::io::stdout();
                let mut w = std::io::BufWriter::with_capacity(1 << 20, out.lock());
                match cc_io_ifc::engine_step::write_ndjson(&bytes, &mut w) {
                    Err(e) => {
                        drop(w);
                        print_error("StepError", &e.0);
                        ExitCode::from(1)
                    }
                    Ok(Err(e)) => {
                        eprintln!("cadcraft-cli: {e}");
                        ExitCode::from(1)
                    }
                    Ok(Ok(())) => {
                        if w.flush().is_err() {
                            return Some(ExitCode::from(1));
                        }
                        ExitCode::SUCCESS
                    }
                }
            }
        }),
        "ifc-geom" => Some(match read(file) {
            Err(c) => c,
            Ok(bytes) => match geom(bytes) {
                Ok(json) => {
                    println!("{json}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    print_error("Error", &e);
                    ExitCode::from(1)
                }
            },
        }),
        _ => None,
    }
}

/// The ifc-geom engine on a thread with a deep stack (boolean trees nest as deep as the file makes them).
fn geom(bytes: Vec<u8>) -> Result<String, String> {
    std::thread::Builder::new()
        .stack_size(512 << 20)
        .spawn(move || {
            cc_io_ifc::engine_geom::products(&bytes, &cc_io_ifc::geo::bool_ops::ManifoldOps).map(|p| cc_io_ifc::engine_geom::to_json(&p))
        })
        .map_err(|e| e.to_string())?
        .join()
        .map_err(|_| "the geometry thread panicked".to_string())?
        .map_err(|e| e.0)
}

/// `bench <engine> <file> --runs N` for the IFC engines; None when the engine is not one of them.
pub fn bench(engine: &str, file: &str, runs: usize) -> Option<ExitCode> {
    let run: fn(&[u8]) -> Result<(), String> = match engine {
        "ifc-step" => |b| cc_io_ifc::engine_step::run_all(b).map(drop).map_err(|e| e.0),
        "ifc-geom" => |b| cc_io_ifc::engine_geom::products(b, &cc_io_ifc::geo::bool_ops::ManifoldOps).map(drop).map_err(|e| e.0),
        _ => return None,
    };
    let bytes = match read(file) {
        Ok(b) => b,
        Err(c) => return Some(c),
    };
    if let Err(e) = run(&bytes) {
        print_error("refused", &e);
        return Some(ExitCode::from(1));
    }
    let mut open = Vec::new();
    let mut parse = Vec::new();
    for _ in 0..runs.max(1) {
        let t = Instant::now();
        let bytes = match read(file) {
            Ok(b) => b,
            Err(c) => return Some(c),
        };
        open.push(t.elapsed().as_secs_f64() * 1000.0);
        let t = Instant::now();
        let r = run(&bytes);
        parse.push(t.elapsed().as_secs_f64() * 1000.0);
        drop(r);
    }
    let arr = |v: &[f64]| format!("[{}]", v.iter().map(|x| format!("{:.3}", x)).collect::<Vec<_>>().join(","));
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    println!(
        r#"{{"open_ms":{},"parse_ms":{},"first_frame_ms":null,"runtime":"rust {} native, rayon {} threads"}}"#,
        arr(&open),
        arr(&parse),
        env!("CARGO_PKG_VERSION"),
        threads
    );
    Some(ExitCode::SUCCESS)
}
