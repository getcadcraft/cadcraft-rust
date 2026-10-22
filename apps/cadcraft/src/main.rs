//! CadCraft desktop (Rust test version).
//!
//!   cadcraft [file] [--control [PORT]] [--allow read,edit,save,export]
//!
//! `--control` (or CADCRAFT_CONTROL_PORT / APP_CONTROL_PORT) starts the local control channel for AI assistants and
//! scripts (control.rs): 127.0.0.1 only, port 47615 by default, a bearer token printed on stderr and written to
//! `control.token` in the config folder; `--allow` names the permission groups (default read).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod control;

use cc_ui_egui::{ImportedFile, Services, UiApp};

struct App {
    app: UiApp,
    /// --bench: print the result and quit
    bench_exit: bool,
    /// the control channel, when started
    control: Option<control::Channel>,
}

/// The start screen's recent files, kept in `recent.json` in the config folder between runs.
fn recent_path() -> Option<std::path::PathBuf> {
    cc_mcp::token::config_dir().map(|d| d.join("recent.json"))
}

fn load_recent(app: &mut UiApp) {
    let Some(p) = recent_path() else { return };
    let Ok(text) = std::fs::read(&p) else { return };
    // a damaged file is ignored (and replaced on the next save); at most 1 MB is read
    if text.len() > 1 << 20 {
        return;
    }
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&text) else { return };
    for r in v.as_array().map(Vec::as_slice).unwrap_or(&[]).iter().take(cc_ui_egui::hub::MAX_RECENT) {
        let (Some(name), Some(slug)) = (r.get("name").and_then(|x| x.as_str()), r.get("app").and_then(|x| x.as_str())) else { continue };
        app.hub.recent.push(cc_ui_egui::hub::Recent {
            name: name.chars().take(260).collect(),
            dir: r.get("dir").and_then(|x| x.as_str()).map(str::to_string),
            app: cc_ui_egui::hub::app_static(slug),
            when: r.get("when").and_then(serde_json::Value::as_f64).unwrap_or(0.0),
        });
    }
}

fn save_recent(app: &UiApp) {
    let Some(p) = recent_path() else { return };
    let list: Vec<serde_json::Value> =
        app.hub.recent.iter().map(|r| serde_json::json!({"name": r.name, "dir": r.dir, "app": r.app, "when": r.when})).collect();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = cc_mcp::confine::write_file_atomic(&p, serde_json::Value::Array(list).to_string().as_bytes());
}

/// What MarkupCraft keeps between runs (tool chest, picture, signature, tool settings): `markupcraft.json` in the config
/// folder (the JS keeps them in the browser's localStorage).
fn mc_prefs_path() -> Option<std::path::PathBuf> {
    cc_mcp::token::config_dir().map(|d| d.join("markupcraft.json"))
}

fn load_mc_prefs(app: &mut UiApp) {
    let Some(p) = mc_prefs_path() else { return };
    let Ok(text) = std::fs::read(&p) else { return };
    // a damaged file is ignored (and replaced on the next save); at most 16 MB is read (pictures travel as data URLs)
    if text.len() > 16 << 20 {
        return;
    }
    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&text) {
        app.mc_prefs = v;
    }
}

/// CadCraft's own preferences (SHORTCUTS' keys), as 0.5 keeps `cadcraft.shortcuts`.
fn cad_prefs_path() -> Option<std::path::PathBuf> {
    cc_mcp::token::config_dir().map(|d| d.join("cadcraft.json"))
}

fn load_cad_prefs(app: &mut UiApp) {
    let Some(p) = cad_prefs_path() else { return };
    let Ok(text) = std::fs::read(&p) else { return };
    if text.len() > 1 << 20 {
        return;
    }
    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&text) {
        app.winset.load_prefs(&v);
    }
}

fn save_cad_prefs(app: &UiApp) {
    let Some(p) = cad_prefs_path() else { return };
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = cc_mcp::confine::write_file_atomic(&p, app.winset.prefs_json().to_string().as_bytes());
}

fn save_mc_prefs(app: &UiApp) {
    let Some(p) = mc_prefs_path() else { return };
    if app.mc_prefs.is_null() {
        return;
    }
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = cc_mcp::confine::write_file_atomic(&p, app.mc_prefs.to_string().as_bytes());
}

impl eframe::App for App {
    fn on_exit(&mut self) {
        save_recent(&self.app);
        save_mc_prefs(&self.app);
        save_cad_prefs(&self.app);
    }
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.app.raw_input_hook(raw);
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(c) = self.control.as_mut() {
            c.pump(&mut self.app);
        }
        self.app.ui(ui);
        if self.bench_exit
            && let Some(r) = self.app.bench_result.take()
        {
            eprintln!("{r}");
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn pick_open() -> Option<Result<ImportedFile, String>> {
    let path = rfd::FileDialog::new()
        .add_filter(
            "Drawings and models",
            &[
                "dwg",
                "dxf",
                "DWG",
                "DXF",
                "modelcraft",
                "ifc",
                "IFC",
                "skp",
                "SKP",
                "obj",
                "glb",
                "gltf",
                "dae",
                "pdf",
                "PDF",
                "csv",
                "txt",
                "xml",
                "landxml",
                "json",
            ],
        )
        .pick_file()?;
    Some(read_path(&path))
}

fn read_path(path: &std::path::Path) -> Result<ImportedFile, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let dir = path.parent().map(|p| p.to_string_lossy().into_owned());
    std::fs::read(path).map(|bytes| ImportedFile { name, bytes, dir }).map_err(|e| format!("{}: {e}", path.display()))
}

/// A command's files (XATTACH: a DWG / DXF; IMAGEATTACH / IMAGE: PNG / JPEG pictures, several for IMAGE): a file dialog,
/// then the reads. Cancelled = an empty list.
fn pick_files(kind: &str, multiple: bool) -> Option<Vec<ImportedFile>> {
    let d = if kind == "pictures" {
        rfd::FileDialog::new().add_filter("Pictures", &["png", "jpg", "jpeg", "PNG", "JPG", "JPEG"])
    } else if kind == "library" {
        rfd::FileDialog::new().add_filter("Library items", &["json", "JSON"])
    } else {
        rfd::FileDialog::new().add_filter("Drawings", &["dwg", "dxf", "DWG", "DXF"])
    };
    let paths = if multiple { d.pick_files().unwrap_or_default() } else { d.pick_file().into_iter().collect() };
    Some(paths.iter().filter_map(|p| read_path(p).ok()).collect())
}

/// Save: a save dialog with the suggested name, then the write (None = cancelled).
fn save_file(name: &str, bytes: Vec<u8>) -> Result<Option<String>, String> {
    let ext = name.rsplit_once('.').map_or("dwg", |(_, e)| e).to_string();
    let Some(path) = rfd::FileDialog::new().set_file_name(name).add_filter(ext.to_uppercase(), &[ext.as_str()]).save_file() else {
        return Ok(None);
    };
    std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Some(path.display().to_string()))
}

/// The Plot dialog's "Open to print": the PDF goes to this run's own temp folder and opens in the system's PDF viewer
/// (a fixed program per platform with the file as its one argument; nothing from the drawing chooses what runs).
fn open_pdf(name: &str, bytes: &[u8]) -> Result<bool, String> {
    let dir = std::env::temp_dir().join(format!("cadcraft-print-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    // the name the plot gives, kept to a plain file name
    let file: String = name.chars().map(|c| if c.is_control() || matches!(c, '/' | '\\' | ':') { '_' } else { c }).collect();
    let path = dir.join(if file.trim().is_empty() { "plot.pdf".to_string() } else { file });
    std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let viewer = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(viewer).arg(&path).spawn().map_err(|e| format!("{viewer}: {e}"))?;
    Ok(true)
}

/// "<xref>.dwg" in the drawing's folder, case-insensitively (as BimCraft looks for linked drawings).
fn read_xref(dir: &str, name: &str) -> Option<Vec<u8>> {
    let want = format!("{name}.dwg").to_lowercase();
    let entry = std::fs::read_dir(dir).ok()?.filter_map(Result::ok).find(|e| e.file_name().to_string_lossy().to_lowercase() == want)?;
    std::fs::read(entry.path()).ok()
}

/// `--control [PORT]` / `--control=PORT`, else the environment; None = off.
fn control_port(args: &[String]) -> Option<u16> {
    for (i, a) in args.iter().enumerate() {
        if let Some(v) = a.strip_prefix("--control=") {
            return v.parse().ok();
        }
        if a == "--control" {
            return Some(args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(cc_mcp::http::DEFAULT_PORT));
        }
    }
    ["CADCRAFT_CONTROL_PORT", "APP_CONTROL_PORT"].iter().find_map(|k| std::env::var(k).ok()).and_then(|v| v.parse().ok())
}

/// `--allow read,edit,save,export` (or `all`); default read. Unknown groups are an error.
fn allowed_groups(args: &[String]) -> Result<Vec<String>, String> {
    let given = args.iter().enumerate().find_map(|(i, a)| {
        a.strip_prefix("--allow=").map(str::to_string).or_else(|| (a == "--allow").then(|| args.get(i + 1).cloned().unwrap_or_default()))
    });
    let Some(list) = given else { return Ok(vec!["read".into()]) };
    let mut out: Vec<String> = list.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    if out.iter().any(|g| g == "all") {
        out = cc_mcp::rpc::TOOL_GROUPS.iter().map(|g| g.to_string()).collect();
    }
    if let Some(bad) = out.iter().find(|g| !cc_mcp::rpc::TOOL_GROUPS.contains(&g.as_str())) {
        return Err(format!("Unknown permission \"{bad}\". Groups: read, edit, save, export."));
    }
    Ok(out)
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let bench = args.iter().any(|a| a == "--bench");
    let port = control_port(&args);
    let groups = match allowed_groups(&args) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("cadcraft: {e}");
            std::process::exit(2);
        }
    };
    // values that follow a flag are not files
    let start = args
        .iter()
        .enumerate()
        .find(|(i, a)| {
            !a.starts_with("--") && !matches!(i.checked_sub(1).and_then(|j| args.get(j)).map(String::as_str), Some("--control" | "--allow"))
        })
        .map(|(_, a)| a.clone());
    let mut channel = match port {
        Some(p) => match control::Channel::start(p, groups, cc_mcp::token::config_dir()) {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("cadcraft: the control channel did not start: {e}");
                None
            }
        },
        None => None,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("CadCraft (Rust test version)").with_inner_size([1280.0, 800.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "CadCraft (Rust test version)",
        options,
        Box::new(move |cc| {
            let start_is_none = start.is_none();
            if let Some(c) = channel.as_ref() {
                *c.ctx.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(cc.egui_ctx.clone());
            }
            let control_line = channel.as_ref().map(control::Channel::status_line);
            if let Some(rs) = &cc.wgpu_render_state {
                cc_ui_egui::install_gpu(rs);
            }
            let services = Services {
                pick_open: Some(Box::new(pick_open)),
                pick_files: Some(Box::new(pick_files)),
                read_xref: Some(Box::new(read_xref)),
                save: Some(Box::new(save_file)),
                open_pdf: Some(Box::new(open_pdf)),
                read_path: Some(Box::new(|p: &str| read_path(std::path::Path::new(p)))),
                ..Default::default()
            };
            // big parts rebuild on a thread in the window (PartCraft; headless runs rebuild in place)
            cc_ui_egui::partview::BACKGROUND_REBUILDS.store(true, std::sync::atomic::Ordering::Relaxed);
            let mut app = UiApp::new(services);
            load_recent(&mut app);
            load_mc_prefs(&mut app);
            load_cad_prefs(&mut app);
            if let Some(l) = control_line {
                app.message = l;
            }
            if let Some(p) = start {
                match read_path(std::path::Path::new(&p)) {
                    Ok(f) => app.open_file(&f),
                    Err(e) => app.error = Some(e),
                }
            }
            if bench {
                if start_is_none {
                    let _ = app.run("file.sample", &cc_ui_egui::Params::default());
                }
                app.start_bench(240);
            }
            Ok(Box::new(App { app, bench_exit: bench, control: channel.take() }))
        }),
    )
}
