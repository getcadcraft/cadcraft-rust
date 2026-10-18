//! `cargo xtask <task>`: the quality gate (AGENTS.md section 11) and project chores.
//!
//!   ci            fmt, clippy, test, assets, layers, wasm, deny, fuzz --smoke (in that order; stops at the first failure)
//!   layers        each crate depends only on strictly lower layers; nothing below L6 pulls in a UI toolkit
//!   wasm          `cargo check --target wasm32-unknown-unknown` on every crate of layers L0-L6
//!   assets        every file under assets/ has a row in ASSETS.md
//!   deny          `cargo deny check`
//!   fuzz [--smoke] [--secs N]   each fuzz target over its corpus (needs the nightly toolchain + cargo-fuzz)
//!   web           trunk build --release of apps/cadcraft-web into dist/web, then the inline init script moves into
//!                 its own hashed file (the host's CSP is script-src 'self' 'wasm-unsafe-eval': no inline script)
#![forbid(unsafe_code)]
// No indexing_slicing deny here: `serde_json::Value[...]` returns Null for a missing key, it never panics.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

type R = Result<(), String>;

/// Crates a layer below the UI may never depend on, directly or through anything else.
const UI_TOOLKITS: &[&str] = &["egui", "eframe", "winit", "rfd", "egui-wgpu", "egui_extras", "arboard"];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let task = args.first().map(String::as_str).unwrap_or("help");
    let rest = args.get(1..).unwrap_or(&[]);
    let res = match task {
        "ci" => ci(),
        "layers" => layers(),
        "wasm" => wasm(),
        "assets" => assets(),
        "deny" => deny(),
        "fuzz" => fuzz(rest),
        "web" => web(),
        _ => {
            eprintln!("usage: cargo xtask <ci|layers|wasm|assets|deny|fuzz [--smoke] [--secs N]|web>");
            return ExitCode::from(2);
        }
    };
    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask {task}: FAILED: {e}");
            ExitCode::FAILURE
        }
    }
}

fn root() -> PathBuf {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    here.parent().map(Path::to_path_buf).unwrap_or(here)
}

fn cargo() -> Command {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(root());
    c
}

fn run(name: &str, mut c: Command) -> R {
    eprintln!("== {name}");
    let st = c.status().map_err(|e| format!("{name}: {e}"))?;
    if st.success() { Ok(()) } else { Err(format!("{name} exited with {st}")) }
}

fn ci() -> R {
    let mut fmt = cargo();
    fmt.args(["fmt", "--all", "--check"]);
    run("fmt", fmt)?;
    let mut clippy = cargo();
    clippy.args(["clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings"]);
    run("clippy", clippy)?;
    let mut test = cargo();
    test.args(["test", "--workspace", "--locked", "--quiet"]);
    run("test", test)?;
    assets()?;
    layers()?;
    wasm()?;
    deny()?;
    fuzz(&["--smoke".to_string()])
}

struct Pkg {
    name: String,
    layer: Option<i64>,
    id: String,
}

fn metadata() -> Result<serde_json::Value, String> {
    let out = cargo().args(["metadata", "--format-version", "1", "--locked"]).output().map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata: {}", String::from_utf8_lossy(&out.stderr)));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata json: {e}"))
}

fn workspace_pkgs(meta: &serde_json::Value) -> Result<Vec<Pkg>, String> {
    let members: BTreeSet<&str> =
        meta["workspace_members"].as_array().ok_or("no workspace_members")?.iter().filter_map(|v| v.as_str()).collect();
    let mut pkgs = Vec::new();
    for p in meta["packages"].as_array().ok_or("no packages")? {
        let id = p["id"].as_str().unwrap_or_default();
        if !members.contains(id) {
            continue;
        }
        let name = p["name"].as_str().unwrap_or_default().to_string();
        let layer_v = &p["metadata"]["layer"]["layer"];
        let layer = layer_v.as_i64();
        if layer.is_none() && layer_v.as_str() != Some("tool") {
            return Err(format!("{name}: [package.metadata.layer] layer is missing"));
        }
        pkgs.push(Pkg { name, layer, id: id.to_string() });
    }
    Ok(pkgs)
}

fn layers() -> R {
    eprintln!("== layers");
    let meta = metadata()?;
    let pkgs = workspace_pkgs(&meta)?;
    let by_id: BTreeMap<&str, &Pkg> = pkgs.iter().map(|p| (p.id.as_str(), p)).collect();
    // resolve graph: id -> dependency ids (normal + build deps; dev-deps are tests only)
    let mut graph: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut names: BTreeMap<String, String> = BTreeMap::new();
    for p in meta["packages"].as_array().ok_or("no packages")? {
        if let (Some(id), Some(n)) = (p["id"].as_str(), p["name"].as_str()) {
            names.insert(id.to_string(), n.to_string());
        }
    }
    for node in meta["resolve"]["nodes"].as_array().ok_or("no resolve")? {
        let id = node["id"].as_str().unwrap_or_default().to_string();
        let mut deps = Vec::new();
        for d in node["deps"].as_array().into_iter().flatten() {
            let normal = d["dep_kinds"].as_array().into_iter().flatten().any(|k| k["kind"].as_str() != Some("dev"));
            if normal && let Some(pid) = d["pkg"].as_str() {
                deps.push(pid.to_string());
            }
        }
        graph.insert(id, deps);
    }
    let mut problems = Vec::new();
    for p in &pkgs {
        let Some(layer) = p.layer else { continue };
        for d in graph.get(&p.id).into_iter().flatten() {
            if let Some(dp) = by_id.get(d.as_str()) {
                match dp.layer {
                    Some(dl) if dl < layer => {}
                    Some(dl) => problems.push(format!("{} (L{layer}) depends on {} (L{dl}): only strictly lower layers", p.name, dp.name)),
                    None => problems.push(format!("{} depends on the tool crate {}", p.name, dp.name)),
                }
            }
        }
        if layer < 6 {
            // everything reachable from this crate
            let mut seen = BTreeSet::new();
            let mut stack = vec![p.id.clone()];
            while let Some(cur) = stack.pop() {
                for d in graph.get(&cur).into_iter().flatten() {
                    if seen.insert(d.clone()) {
                        stack.push(d.clone());
                    }
                }
            }
            for s in &seen {
                let n = names.get(s).map(String::as_str).unwrap_or_default();
                if UI_TOOLKITS.contains(&n) {
                    problems.push(format!("{} (L{layer}) pulls in the UI toolkit {n}", p.name));
                }
            }
        }
    }
    if problems.is_empty() {
        eprintln!("   {} crates, layering ok", pkgs.len());
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

fn wasm() -> R {
    let meta = metadata()?;
    let pkgs = workspace_pkgs(&meta)?;
    let mut c = cargo();
    c.args(["check", "--locked", "--target", "wasm32-unknown-unknown"]);
    let mut n = 0;
    for p in pkgs.iter().filter(|p| matches!(p.layer, Some(l) if l <= 6)) {
        c.args(["-p", &p.name]);
        n += 1;
    }
    // the web entry too: a new field in the UI's Services once broke only `cargo xtask web` (M16.7)
    c.args(["-p", "cadcraft-web"]);
    run(&format!("wasm ({} crates + cadcraft-web)", n), c)
}

fn assets() -> R {
    eprintln!("== assets");
    let dir = root().join("assets");
    let list = std::fs::read_to_string(root().join("ASSETS.md")).map_err(|e| format!("ASSETS.md: {e}"))?;
    let mut files = Vec::new();
    walk(&dir, &mut files);
    let missing: Vec<String> = files
        .iter()
        .filter_map(|f| f.strip_prefix(root()).ok().map(|p| p.to_string_lossy().replace('\\', "/")))
        .filter(|rel| !list.contains(rel.as_str()))
        .collect();
    if missing.is_empty() {
        eprintln!("   {} assets, all listed", files.len());
        Ok(())
    } else {
        Err(format!("files in assets/ with no ASSETS.md row:\n{}", missing.join("\n")))
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.file_name().is_some_and(|n| n != ".keep") {
            out.push(p);
        }
    }
}

fn deny() -> R {
    let mut c = cargo();
    c.args(["deny", "--locked", "check"]);
    run("deny", c)
}

fn fuzz(args: &[String]) -> R {
    let smoke = args.iter().any(|a| a == "--smoke");
    let secs: u64 = args
        .iter()
        .position(|a| a == "--secs")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(if smoke { 30 } else { 600 });
    let dir = root().join("fuzz").join("fuzz_targets");
    let mut targets: Vec<String> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "rs"))
        .filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    targets.sort();
    if targets.is_empty() {
        eprintln!("== fuzz: no targets yet");
        return Ok(());
    }
    for t in targets {
        let corpus = root().join("fuzz").join("corpus").join(&t);
        let _ = std::fs::create_dir_all(&corpus);
        let seeds = root().join("fuzz").join("seeds").join(&t);
        let mut c = Command::new("cargo");
        c.current_dir(root().join("fuzz"));
        c.args(["+nightly", "fuzz", "run", &t]);
        c.arg(&corpus);
        if seeds.is_dir() {
            c.arg(&seeds);
        }
        c.args(["--", &format!("-max_total_time={secs}"), "-rss_limit_mb=2048", "-max_len=4194304"]);
        run(&format!("fuzz {t} ({secs} s)"), c)?;
    }
    Ok(())
}

fn web() -> R {
    let app = root().join("apps").join("cadcraft-web");
    let mut c = Command::new("trunk");
    c.current_dir(&app);
    c.args(["build", "--release"]);
    run("trunk build --release", c)?;
    let dist = root().join("dist").join("web");
    let index = dist.join("index.html");
    let html = std::fs::read_to_string(&index).map_err(|e| format!("{}: {e}", index.display()))?;
    let Some(a) = html.find("<script type=\"module\">") else { return Ok(()) };
    let body_start = a + "<script type=\"module\">".len();
    let Some(rel_end) = html.get(body_start..).and_then(|t| t.find("</script>")) else {
        return Err("index.html: unterminated inline script".into());
    };
    let body = html.get(body_start..body_start + rel_end).unwrap_or_default();
    // name the file after its content (FNV-1a, 16 hex digits) so it can be cached forever
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in body.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    let name = format!("init-{h:016x}.js");
    std::fs::write(dist.join(&name), body).map_err(|e| format!("{name}: {e}"))?;
    let tag = format!("<script type=\"module\" src=\"/{name}\"></script>");
    let out =
        format!("{}{}{}", html.get(..a).unwrap_or_default(), tag, html.get(body_start + rel_end + "</script>".len()..).unwrap_or_default());
    std::fs::write(&index, out).map_err(|e| format!("{}: {e}", index.display()))?;
    eprintln!("   inline init script → {name}");
    Ok(())
}
