//! CadCraft desktop (Rust test version): the 2D viewer.
//!
//!   cadcraft [file.dwg|file.dxf]
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

use cc_ui_egui::{ImportedFile, Services, UiApp};

struct App {
    app: UiApp,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
    }
}

fn pick_open() -> Option<Result<ImportedFile, String>> {
    let path = rfd::FileDialog::new().add_filter("Drawings", &["dwg", "dxf", "DWG", "DXF"]).pick_file()?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Some(std::fs::read(&path).map(|bytes| ImportedFile { name, bytes }).map_err(|e| format!("{}: {e}", path.display())))
}

fn main() -> eframe::Result {
    let start = std::env::args().nth(1);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("CadCraft (Rust test version)").with_inner_size([1280.0, 800.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "CadCraft (Rust test version)",
        options,
        Box::new(move |cc| {
            if let Some(rs) = &cc.wgpu_render_state {
                cc_ui_egui::install_gpu(rs);
            }
            let services = Services { pick_open: Some(Box::new(pick_open)), ..Default::default() };
            let mut app = UiApp::new(services);
            if let Some(p) = start {
                match std::fs::read(&p) {
                    Ok(b) => app.open_bytes(&p, &b),
                    Err(e) => app.error = Some(format!("{p}: {e}")),
                }
            }
            Ok(Box::new(App { app }))
        }),
    )
}
