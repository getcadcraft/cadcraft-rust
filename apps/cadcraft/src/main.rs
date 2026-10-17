//! CadCraft desktop (Rust test version): the 2D viewer.
//!
//!   cadcraft [file.dwg|file.dxf]
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

use cc_ui_egui::{ImportedFile, Services, UiApp};

struct App {
    app: UiApp,
    /// --bench: print the result and quit
    bench_exit: bool,
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.app.raw_input_hook(raw);
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
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

/// Save: a save dialog with the suggested name, then the write (None = cancelled).
fn save_file(name: &str, bytes: Vec<u8>) -> Result<Option<String>, String> {
    let ext = name.rsplit_once('.').map_or("dwg", |(_, e)| e).to_string();
    let Some(path) = rfd::FileDialog::new().set_file_name(name).add_filter(ext.to_uppercase(), &[ext.as_str()]).save_file() else {
        return Ok(None);
    };
    std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Some(path.display().to_string()))
}

/// "<xref>.dwg" in the drawing's folder, case-insensitively (as BimCraft looks for linked drawings).
fn read_xref(dir: &str, name: &str) -> Option<Vec<u8>> {
    let want = format!("{name}.dwg").to_lowercase();
    let entry = std::fs::read_dir(dir).ok()?.filter_map(Result::ok).find(|e| e.file_name().to_string_lossy().to_lowercase() == want)?;
    std::fs::read(entry.path()).ok()
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let bench = args.iter().any(|a| a == "--bench");
    let start = args.into_iter().find(|a| !a.starts_with("--"));
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
            if let Some(rs) = &cc.wgpu_render_state {
                cc_ui_egui::install_gpu(rs);
            }
            let services = Services {
                pick_open: Some(Box::new(pick_open)),
                read_xref: Some(Box::new(read_xref)),
                save: Some(Box::new(save_file)),
                ..Default::default()
            };
            let mut app = UiApp::new(services);
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
            Ok(Box::new(App { app, bench_exit: bench }))
        }),
    )
}
