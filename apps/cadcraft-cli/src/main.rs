//! cadcraft-cli: the headless CadCraft (Rust test version).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

fn main() -> std::process::ExitCode {
    eprintln!("usage: cadcraft-cli <command> (commands arrive in milestone M2)");
    std::process::ExitCode::from(2)
}
