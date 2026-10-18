//! `cadcraft-cli mcp`: CadCraft's tools for an AI assistant over stdio (0.5 tools/mcp/server.mjs; docs/AGENTS-SPEC.md
//! § 5.1; AGENTS.md § 9). Newline-delimited JSON-RPC 2.0 on stdin / stdout, anything else on stderr.
//!
//!   cadcraft-cli mcp [--root DIR]… [--automation-read-root DIR]… [--automation-write-root DIR]… [--allow read,edit,save,export]
//!                    [--log FILE.jsonl] [--verbose]
//!   cadcraft-cli mcp --connect PORT --token-file FILE        relay to a running app's control channel
//!
//! Files only inside the named folders (the current folder when none is named: confinement is on by default); tools only
//! from the --allow groups (default read). CADCRAFT_MCP_ROOTS / CADCRAFT_MCP_ALLOW work as in 0.5.

use std::io::{BufRead, Write};
use std::process::ExitCode;

use cc_mcp::confine::Roots;
use cc_mcp::headless::Headless;
use cc_mcp::rpc::{Dispatcher, Session, TOOL_GROUPS};
use serde_json::{Value, json};

const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The longest message line read (AGENTS.md § 9's cap for the control channel; a longer line is refused).
const MAX_LINE: usize = 4 * 1024 * 1024;

const HELP: &str = "cadcraft-cli mcp — CadCraft's tools for an AI assistant (MCP over stdio)

  cadcraft-cli mcp [--root <folder> …] [--allow read,edit,save,export] [--log <file.jsonl>] [--verbose]
                   [--automation-read-root <folder> …] [--automation-write-root <folder> …]
  cadcraft-cli mcp --connect <port> --token-file <file>

  --root     a folder the assistant may open files in and write to (repeat for more). Without any folder option the
             current folder is the only one (files elsewhere are refused).
  --automation-read-root / --automation-write-root   a folder for reading only / for writing (and reading back)
  --allow    permission groups (default read):
               read    open, describe, list, query and check drawings, IFC models, takeoffs and PDF markups
               edit    run commands and scripts on open drawings (each call one undo step)
               save    write drawings (DWG/DXF) back, or as new drawing files
               export  write converted drawings, CSV/XLSX
  --log      append every call as a JSON line (inside an allowed folder or the current folder)
  --verbose  also print every call on stderr
  --connect  relay to the control channel of a running CadCraft (cadcraft --control); the token comes from --token-file

  Or in the environment: CADCRAFT_MCP_ROOTS (folders separated by the system's path separator) and CADCRAFT_MCP_ALLOW.
";

#[derive(Default)]
struct Opts {
    roots: Vec<String>,
    read_roots: Vec<String>,
    write_roots: Vec<String>,
    allow: Vec<String>,
    allow_given: bool,
    log: Option<String>,
    verbose: bool,
    help: bool,
    version: bool,
    connect: Option<u16>,
    token_file: Option<String>,
}

fn parse(argv: &[String]) -> Result<Opts, String> {
    let mut o = Opts { allow: vec!["read".into()], ..Default::default() };
    let mut i = 0usize;
    while i < argv.len() {
        let a = argv.get(i).cloned().unwrap_or_default();
        let mut val = || -> Result<String, String> {
            i += 1;
            argv.get(i).cloned().ok_or_else(|| format!("{a} needs a value."))
        };
        let split = |s: &str| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect::<Vec<_>>();
        match a.as_str() {
            "--root" | "-r" => o.roots.push(val()?),
            "--automation-read-root" => o.read_roots.push(val()?),
            "--automation-write-root" => o.write_roots.push(val()?),
            "--allow" => {
                o.allow = split(&val()?);
                o.allow_given = true;
            }
            "--log" => o.log = Some(val()?),
            "--verbose" | "-v" => o.verbose = true,
            "--help" | "-h" => o.help = true,
            "--version" => o.version = true,
            "--connect" => o.connect = Some(val()?.parse().map_err(|_| "--connect needs a port number.".to_string())?),
            "--token-file" => o.token_file = Some(val()?),
            s if s.starts_with("--root=") => o.roots.push(s["--root=".len()..].to_string()),
            s if s.starts_with("--allow=") => {
                o.allow = split(&s["--allow=".len()..]);
                o.allow_given = true;
            }
            s if s.starts_with("--automation-read-root=") => o.read_roots.push(s["--automation-read-root=".len()..].to_string()),
            s if s.starts_with("--automation-write-root=") => o.write_roots.push(s["--automation-write-root=".len()..].to_string()),
            _ => return Err(format!("Unknown option {a}. See --help.")),
        }
        i += 1;
    }
    if o.roots.is_empty()
        && let Some(r) = std::env::var_os("CADCRAFT_MCP_ROOTS")
    {
        o.roots = std::env::split_paths(&r).map(|p| p.display().to_string()).filter(|s| !s.is_empty()).collect();
    }
    if !o.allow_given
        && let Ok(a) = std::env::var("CADCRAFT_MCP_ALLOW")
    {
        o.allow = a.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
    }
    if o.allow.iter().any(|g| g == "all") {
        o.allow = TOOL_GROUPS.iter().map(|g| g.to_string()).collect();
    }
    if let Some(g) = o.allow.iter().find(|g| !TOOL_GROUPS.contains(&g.as_str())) {
        return Err(format!("Unknown permission \"{g}\". Groups: {}.", TOOL_GROUPS.join(", ")));
    }
    Ok(o)
}

/// One line of at most MAX_LINE bytes (without the newline); Ok(None) at the end; Err(()) = too long (skipped).
fn read_line(r: &mut impl BufRead) -> std::io::Result<Option<Result<String, ()>>> {
    let mut buf = Vec::new();
    let n = std::io::Read::take(r.by_ref(), (MAX_LINE as u64) + 1).read_until(b'\n', &mut buf)?;
    if n == 0 {
        return Ok(None);
    }
    if buf.last() != Some(&b'\n') && buf.len() > MAX_LINE {
        // skip the rest of the line
        let mut sink = Vec::new();
        loop {
            sink.clear();
            let k = std::io::Read::take(r.by_ref(), 1 << 20).read_until(b'\n', &mut sink)?;
            if k == 0 || sink.last() == Some(&b'\n') {
                break;
            }
        }
        return Ok(Some(Err(())));
    }
    while matches!(buf.last(), Some(b'\n' | b'\r')) {
        buf.pop();
    }
    Ok(Some(Ok(String::from_utf8_lossy(&buf).into_owned())))
}

fn send(out: &mut impl Write, v: &Value) {
    let _ = writeln!(out, "{}", cc_mcp::json::stringify(v));
    let _ = out.flush();
}

pub fn run(argv: &[String]) -> ExitCode {
    let o = match parse(argv) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("cadcraft-mcp: {e}");
            return ExitCode::from(2);
        }
    };
    if o.help {
        print!("{HELP}");
        return ExitCode::SUCCESS;
    }
    if o.version {
        println!("{VERSION}");
        return ExitCode::SUCCESS;
    }
    if let Some(port) = o.connect {
        return relay(port, o.token_file.as_deref());
    }
    let roots = match Roots::new(&o.roots, &o.read_roots, &o.write_roots) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("cadcraft-mcp: {}", e.message);
            return ExitCode::from(2);
        }
    };
    let folders: Vec<String> = roots.list().iter().map(|p| p.display().to_string()).collect();
    let mut log_file: Option<std::path::PathBuf> = None;
    if let Some(l) = &o.log {
        let abs = std::path::absolute(l).unwrap_or_else(|_| std::path::PathBuf::from(l));
        let dir = abs.parent().and_then(|d| std::fs::canonicalize(d).ok());
        let cwd = std::env::current_dir().ok().and_then(|c| std::fs::canonicalize(c).ok());
        let ok = dir.as_ref().is_some_and(|d| roots.write_list().iter().any(|r| d.starts_with(r)) || Some(d) == cwd.as_ref());
        match (ok, dir, abs.file_name()) {
            (true, Some(d), Some(n)) => log_file = Some(d.join(n)),
            _ => {
                eprintln!("cadcraft-mcp: --log must be inside an allowed folder or the current folder.");
                return ExitCode::from(2);
            }
        }
    }
    let allowed = o.allow.clone();
    let mut d = Dispatcher::new("cadcraft", "CadCraft", VERSION);
    d.allowed = allowed.clone();
    let a2 = allowed.clone();
    d.how_to_allow = Some(Box::new(move |g: &str| {
        let mut all = a2.clone();
        if !all.iter().any(|x| x == g) {
            all.push(g.to_string());
        }
        format!("The person can restart the server with --allow {}.", all.join(","))
    }));
    d.instructions = Some(format!(
        "CadCraft tools for DWG/DXF drawings (CadCraft), IFC models (BimCraft), takeoffs (TakeoffCraft) and PDF markups (MarkupCraft). Files: only inside {}. Allowed: {}. Open a file first (drawing_open, ifc_open, takeoff_open) and pass the handle it returns. command_run types at CadCraft's command line, one prompt answer per list item; every edit is one undo step labelled \"Assistant: …\".",
        if roots.any() { folders.join(", ") } else { "no folders (no --root was given)".to_string() },
        allowed.join(", ")
    ));
    let verbose = o.verbose;
    d.log = Some(Box::new(move |e: &Value| {
        if let Some(p) = &log_file {
            let line = format!("{}\n", cc_mcp::json::stringify(e));
            if let Err(err) = std::fs::OpenOptions::new().create(true).append(true).open(p).and_then(|mut f| f.write_all(line.as_bytes())) {
                eprintln!("cadcraft-mcp: could not write the log: {err}");
            }
        }
        if verbose {
            let msg = e.get("message").and_then(Value::as_str).map(|m| format!(" — {m}")).unwrap_or_default();
            eprintln!(
                "[{}] {} {} {} ms{msg}",
                e["time"].as_str().unwrap_or(""),
                e["tool"].as_str().unwrap_or(""),
                e["outcome"].as_str().unwrap_or(""),
                cc_mcp::json::js_string(&e["ms"])
            );
        }
    }));
    let mut tools = Headless::new(roots);
    eprintln!(
        "cadcraft-mcp {VERSION}: folders {}; allowed: {}.",
        if folders.is_empty() { "(none — no file access)".to_string() } else { folders.join(", ") },
        allowed.join(", ")
    );
    let mut session = Session::new("stdio");
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    loop {
        let line = match read_line(&mut input) {
            Ok(Some(Ok(l))) => l,
            Ok(Some(Err(()))) => {
                send(&mut out, &json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32600, "message": "Message too large"}}));
                continue;
            }
            Ok(None) | Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            send(&mut out, &json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "Parse error"}}));
            continue;
        };
        if msg.is_array() {
            send(&mut out, &json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32600, "message": "Batches are not supported"}}));
            continue;
        }
        if let Some(a) = d.handle(&msg, &mut session, &mut tools) {
            send(&mut out, &a);
        }
    }
    ExitCode::SUCCESS
}

/// `--connect`: relay stdio to a running app's control channel (POST /mcp with the token; the session id it issues).
fn relay(port: u16, token_file: Option<&str>) -> ExitCode {
    let token = match token_file.map(|p| cc_mcp::token::read_token_file(std::path::Path::new(p))) {
        Some(Ok(t)) => t,
        Some(Err(e)) => {
            eprintln!("cadcraft-mcp: {e}");
            return ExitCode::from(2);
        }
        None => {
            eprintln!("cadcraft-mcp: --connect needs --token-file (the control.token the app wrote).");
            return ExitCode::from(2);
        }
    };
    let auth = format!("Bearer {token}");
    let mut sid: Option<String> = None;
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    loop {
        let line = match read_line(&mut input) {
            Ok(Some(Ok(l))) => l,
            Ok(Some(Err(()))) => {
                send(&mut out, &json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32600, "message": "Message too large"}}));
                continue;
            }
            Ok(None) | Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        let id = serde_json::from_str::<Value>(&line).ok().and_then(|m| m.get("id").cloned()).unwrap_or(Value::Null);
        let mut h: Vec<(&str, &str)> = vec![("Authorization", auth.as_str()), ("Content-Type", "application/json")];
        if let Some(s) = sid.as_deref() {
            h.push(("Mcp-Session-Id", s));
        }
        match cc_mcp::http::request(port, "POST", "/mcp", &h, line.as_bytes()) {
            Ok((status, headers, body)) => {
                if let Some((_, s)) = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("Mcp-Session-Id")) {
                    sid = Some(s.clone());
                }
                if status == 202 || body.is_empty() {
                    continue;
                }
                let _ = out.write_all(&body);
                let _ = out.write_all(b"\n");
                let _ = out.flush();
            }
            Err(e) => send(
                &mut out,
                &json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32603, "message": format!("The CadCraft window could not be reached: {e}")}}),
            ),
        }
    }
    ExitCode::SUCCESS
}
