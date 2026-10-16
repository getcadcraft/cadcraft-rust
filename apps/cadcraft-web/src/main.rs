//! CadCraft web entry (Rust test version): eframe's web runner on WebGPU only (no WebGL2 fallback).
//! A browser without WebGPU gets one clear message (the page shows it until the app starts).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("cadcraft-web is the browser build: trunk build --release (apps/cadcraft-web).");
}

#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}

#[cfg(target_arch = "wasm32")]
mod web {
    use std::sync::{Arc, Mutex, PoisonError};

    use cc_ui_egui::{ImportedFile, Params, Services, UiApp};
    use wasm_bindgen::JsCast;

    const NO_WEBGPU: &str = "This preview needs WebGPU: use Chrome or Edge, Safari 26+, Firefox 141+ on Windows / 145+ on Apple-silicon Macs, or CadCraft Studio.";

    struct App {
        app: UiApp,
    }
    impl eframe::App for App {
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            self.app.ui(ui);
        }
    }

    fn set_status(text: &str) {
        if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("status")) {
            el.set_text_content(Some(text));
        }
    }
    fn hide_status() {
        if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("status")) {
            let _ = el.set_attribute("hidden", "");
        }
    }

    pub fn start() {
        let Some(window) = web_sys::window() else { return };
        // WebGPU only: no navigator.gpu → the message, no broken canvas
        let has_gpu = js_has_webgpu(&window);
        if !has_gpu {
            set_status(NO_WEBGPU);
            return;
        }
        let search = window.location().search().unwrap_or_default();
        let sample = search.split(['?', '&']).any(|p| p == "sample");
        let bench_frames = search.split(['?', '&']).any(|p| p == "bench");
        if bench_frames {
            bench();
        }
        let Some(canvas) =
            window.document().and_then(|d| d.get_element_by_id("cc_canvas")).and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
        else {
            set_status("The page has no canvas (cc_canvas).");
            return;
        };
        let mut options = eframe::WebOptions::default();
        options.wgpu_options.wgpu_setup = match options.wgpu_options.wgpu_setup {
            egui_wgpu::WgpuSetup::CreateNew(mut c) => {
                c.instance_descriptor.backends = wgpu::Backends::BROWSER_WEBGPU;
                egui_wgpu::WgpuSetup::CreateNew(c)
            }
            other => other,
        };
        wasm_bindgen_futures::spawn_local(async move {
            let runner = eframe::WebRunner::new();
            // wasm32 aborts on panic: tell the page the instance stopped (after eframe's own handler)
            let prev = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                prev(info);
                if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("status")) {
                    let _ = el.remove_attribute("hidden");
                    el.set_text_content(Some("The CadCraft test version stopped after an internal error. Reload the page to start again."));
                }
            }));
            let res = runner
                .start(
                    canvas,
                    options,
                    Box::new(move |cc| {
                        if let Some(rs) = &cc.wgpu_render_state {
                            cc_ui_egui::install_gpu(rs);
                        }
                        let inbox: Arc<Mutex<Vec<ImportedFile>>> = Arc::new(Mutex::new(Vec::new()));
                        let ctx = cc.egui_ctx.clone();
                        let open_inbox = Arc::clone(&inbox);
                        let open_ctx = ctx.clone();
                        let drop_inbox = Arc::clone(&inbox);
                        let drop_ctx = ctx.clone();
                        let services = Services {
                            open_async: Some(Box::new(move || {
                                let inbox = Arc::clone(&open_inbox);
                                let ctx = open_ctx.clone();
                                wasm_bindgen_futures::spawn_local(async move {
                                    let Some(f) = rfd::AsyncFileDialog::new().add_filter("Drawings", &["dwg", "dxf"]).pick_file().await
                                    else {
                                        return;
                                    };
                                    let bytes = f.read().await;
                                    inbox.lock().unwrap_or_else(PoisonError::into_inner).push(ImportedFile {
                                        name: f.file_name(),
                                        bytes,
                                        dir: None,
                                    });
                                    ctx.request_repaint();
                                });
                            })),
                            inbox: Some(Arc::clone(&inbox)),
                            read_dropped: Some(Box::new(move |f| {
                                let inbox = Arc::clone(&drop_inbox);
                                let ctx = drop_ctx.clone();
                                wasm_bindgen_futures::spawn_local(async move {
                                    let name = f.path().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                                    if let Ok(bytes) = f.bytes_async().await {
                                        inbox.lock().unwrap_or_else(PoisonError::into_inner).push(ImportedFile { name, bytes, dir: None });
                                        ctx.request_repaint();
                                    }
                                });
                            })),
                            pick_open: None,
                            read_xref: None,
                        };
                        let mut app = UiApp::new(services);
                        if sample || bench_frames {
                            let _ = app.run("file.sample", &Params::default());
                        }
                        if bench_frames {
                            app.start_bench(120);
                        }
                        Ok(Box::new(App { app }))
                    }),
                )
                .await;
            match res {
                Ok(()) => hide_status(),
                Err(_) => set_status(NO_WEBGPU),
            }
        });
    }

    /// `?bench`: time the reader on the built-in sample in this browser (6 runs, like the kit's JS bench) → console.
    fn bench() {
        let mut t = Vec::new();
        for _ in 0..6 {
            let t0 = web_time::Instant::now();
            let m = cc_io_dwg::read_dwg(cc_ui_egui::SAMPLE_DWG, &cc_io_dwg::ReadOpts::default());
            t.push(t0.elapsed().as_secs_f64() * 1000.0);
            drop(m);
        }
        let s: Vec<String> = t.iter().map(|x| format!("{x:.1}")).collect();
        web_sys::console::log_1(&format!("Rust read_dwg ms {}", s.join(" ")).into());
    }

    fn js_has_webgpu(window: &web_sys::Window) -> bool {
        let nav = window.navigator();
        js_sys_has(&nav, "gpu")
    }
    fn js_sys_has(v: &web_sys::Navigator, key: &str) -> bool {
        let obj: &wasm_bindgen::JsValue = v.as_ref();
        js_reflect_has(obj, key)
    }
    #[wasm_bindgen::prelude::wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = Reflect, js_name = has)]
        fn reflect_has(target: &wasm_bindgen::JsValue, key: &wasm_bindgen::JsValue) -> bool;
    }
    fn js_reflect_has(obj: &wasm_bindgen::JsValue, key: &str) -> bool {
        reflect_has(obj, &wasm_bindgen::JsValue::from_str(key))
    }
}
