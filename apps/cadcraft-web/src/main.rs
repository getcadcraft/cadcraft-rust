//! CadCraft web entry (Rust test version): eframe's web runner on WebGPU only (no WebGL2 fallback).
//! A browser without WebGPU gets one clear message (the page shows it until the app starts).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

/// The web build's allocator: wasmalloc (pure Rust, MIT OR Apache-2.0) in place of std's dlmalloc. dlmalloc grows the
/// wasm memory a page or so at a time, and in Chromium each `memory.grow` costs a garbage collection: opening the
/// 3.4 MB Carey drawing spent 45 % of its time in V8's GC and 29 % in `malloc` (CPU profile, 2026-10-27). wasmalloc
/// grows memory geometrically and serves small blocks from size classes: the first open went from 1,320 to 380 ms.
/// It refuses to build with wasm threads (`atomics`); the web build is single-threaded (docs/roadmap.md § Wasm threads).
#[cfg(target_arch = "wasm32")]
#[global_allocator]
static ALLOC: wasmalloc::WasmAlloc = wasmalloc::WasmAlloc::new();

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("cadcraft-web is the browser build: trunk build --release (apps/cadcraft-web).");
}

#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}

#[cfg(target_arch = "wasm32")]
mod recovery;

#[cfg(target_arch = "wasm32")]
mod web {
    use std::sync::{Arc, Mutex, PoisonError};

    use cc_ui_egui::{Fetched, ImportedFile, Params, Services, UiApp};
    use wasm_bindgen::JsCast;

    const SETTINGS_KEY: &str = "cadcraft.rust.settings";

    const NO_WEBGPU: &str = "This preview needs WebGPU: use Chrome or Edge, Safari 26+, Firefox 141+ on Windows / 145+ on Apple-silicon Macs, or CadCraft Studio.";

    struct App {
        app: UiApp,
        /// the drawing shown last frame (its load id): a new one logs its open timings
        shown: u64,
        /// `?bench`: every drawing opened (the sample, or one dropped on the page) runs the frame benchmark
        bench: bool,
        /// the settings last written to the browser's storage
        saved: String,
    }
    impl eframe::App for App {
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            self.app.ui(ui);
            let id = self.app.doc.as_ref().map_or(0, |d| d.id);
            if id != self.shown {
                self.shown = id;
                if let Some(d) = &self.app.doc {
                    log_open(d, self.app.frame_ms);
                    if self.bench {
                        self.app.start_bench(120);
                    }
                }
            }
            if let Some(r) = self.app.bench_result.take() {
                web_sys::console::log_1(&r.into());
            }
            // Settings live in the browser's storage (a per-viewer convenience: nothing breaks without it)
            let now = self.app.settings_json().to_string();
            if now != self.saved {
                if let Some(st) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
                    let _ = st.set_item(SETTINGS_KEY, &now);
                }
                self.saved = now;
            }
        }
        fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
            self.app.raw_input_hook(raw);
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
        let new_drawing = search.split(['?', '&']).any(|p| p == "new");
        let demo3d = search.split(['?', '&']).any(|p| p == "demo3d");
        let model3 = search.split(['?', '&']).any(|p| p == "model");
        let ifc = search.split(['?', '&']).any(|p| p == "ifc");
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
                        // a command's files (XATTACH, IMAGEATTACH, IMAGE): the browser's picker, read into `picked`
                        let picked: Arc<Mutex<Option<Vec<ImportedFile>>>> = Arc::new(Mutex::new(None));
                        let pick_slot = Arc::clone(&picked);
                        let pick_ctx = ctx.clone();
                        let fetched: Arc<Mutex<Vec<Fetched>>> = Arc::new(Mutex::new(Vec::new()));
                        let fetch_out = Arc::clone(&fetched);
                        let fetch_ctx = ctx.clone();
                        let services = Services {
                            pick_files: Some(Box::new(move |kind: &str, multiple: bool| {
                                let slot = Arc::clone(&pick_slot);
                                let ctx = pick_ctx.clone();
                                let pictures = kind == "pictures";
                                let library = kind == "library";
                                let lisp = kind == "lisp";
                                let cloud = kind == "cloud";
                                let gis = kind == "gis";
                                let input = kind == "input";
                                wasm_bindgen_futures::spawn_local(async move {
                                    let d = if lisp {
                                        rfd::AsyncFileDialog::new()
                                            .add_filter("LISP", &["lsp", "dcl", "fas", "vlx", "arx", "crx", "dbx", "dvb", "craft", "lua"])
                                    } else if pictures {
                                        rfd::AsyncFileDialog::new().add_filter("Pictures", &["png", "jpg", "jpeg"])
                                    } else if input {
                                        rfd::AsyncFileDialog::new().add_filter("CadCraft input styles", &["json"])
                                    } else if library {
                                        rfd::AsyncFileDialog::new().add_filter("Library items", &["json"])
                                    } else if cloud {
                                        rfd::AsyncFileDialog::new().add_filter("Point clouds", &["las", "laz", "xyz", "pts"])
                                    } else if gis {
                                        rfd::AsyncFileDialog::new().add_filter(
                                            "GIS files",
                                            &[
                                                "shp", "shx", "dbf", "prj", "cpg", "zip", "geojson", "json", "gpkg", "kml", "kmz", "png",
                                                "jpg", "jpeg", "pgw", "jgw", "wld",
                                            ],
                                        )
                                    } else {
                                        rfd::AsyncFileDialog::new().add_filter("Drawings", &["dwg", "dxf"])
                                    };
                                    let handles = if multiple {
                                        d.pick_files().await.unwrap_or_default()
                                    } else {
                                        d.pick_file().await.into_iter().collect()
                                    };
                                    let mut out = Vec::new();
                                    for h in handles {
                                        out.push(ImportedFile { name: h.file_name(), bytes: h.read().await, dir: None });
                                    }
                                    *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(out);
                                    ctx.request_repaint();
                                });
                                None
                            })),
                            picked: Some(picked),
                            open_async: Some(Box::new(move || {
                                let inbox = Arc::clone(&open_inbox);
                                let ctx = open_ctx.clone();
                                wasm_bindgen_futures::spawn_local(async move {
                                    let Some(f) = rfd::AsyncFileDialog::new()
                                        .add_filter(
                                            "Drawings and models",
                                            &[
                                                "dwg",
                                                "dxf",
                                                "las",
                                                "laz",
                                                "pts",
                                                "xyz",
                                                "modelcraft",
                                                "ifc",
                                                "skp",
                                                "obj",
                                                "glb",
                                                "gltf",
                                                "dae",
                                                "pdf",
                                                "csv",
                                                "txt",
                                                "xml",
                                                "landxml",
                                                "json",
                                            ],
                                        )
                                        .pick_file()
                                        .await
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
                            read_path: None,
                            save: Some(Box::new(download)),
                            open_pdf: Some(Box::new(open_pdf)),
                            pick_folder: None,
                            recovery: Some(crate::recovery::service()),
                            // CivilCraft's map layers (gis-map-ui.js FETCH_OPTS: cors, no credentials, no referrer)
                            fetch: Some(Box::new(move |url: &str| {
                                let out = Arc::clone(&fetch_out);
                                let ctx = fetch_ctx.clone();
                                let url = url.to_string();
                                wasm_bindgen_futures::spawn_local(async move {
                                    let result = fetch_bytes(&url).await;
                                    out.lock().unwrap_or_else(PoisonError::into_inner).push(Fetched { url, result });
                                    ctx.request_repaint();
                                });
                            })),
                            fetched: Some(fetched),
                        };
                        let mut app = UiApp::new(services);
                        // the language: kept setting, else ?lang=, else the browser's (0.5's "Automatic")
                        let kept = web_sys::window()
                            .and_then(|w| w.local_storage().ok().flatten())
                            .and_then(|st| st.get_item(SETTINGS_KEY).ok().flatten())
                            .filter(|t| t.len() <= 1 << 16)
                            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
                        let url_lang = search.split(['?', '&']).find_map(|p| p.strip_prefix("lang=")).and_then(cc_lang::Locale::parse);
                        match (kept, url_lang) {
                            (_, Some(l)) => app.set_locale(l),
                            (Some(v), None) => app.load_settings(&v),
                            (None, None) => {
                                if let Some(l) =
                                    web_sys::window().and_then(|w| w.navigator().language()).and_then(|l| cc_lang::Locale::parse(&l))
                                {
                                    app.set_locale(l);
                                }
                            }
                        }
                        let saved = app.settings_json().to_string();
                        if sample || bench_frames {
                            let _ = app.run("file.sample", &Params::default());
                        }
                        if new_drawing && !sample && !bench_frames {
                            let _ = app.run("file.new", &Params::default());
                        }
                        if demo3d {
                            let _ = app.run("view.demo3d", &Params::default());
                        }
                        if model3 {
                            let _ = app.run("view.model.sample", &Params::default());
                        }
                        if ifc {
                            let _ = app.run("file.sample.ifc", &Params::default());
                        }
                        Ok(Box::new(App { app, shown: 0, bench: bench_frames, saved }))
                    }),
                )
                .await;
            match res {
                Ok(()) => hide_status(),
                Err(_) => set_status(NO_WEBGPU),
            }
        });
    }

    /// A map server's answer: (content type, bytes), at most 32 MB; "http:<status> <text>" or "net:" on failure.
    async fn fetch_bytes(url: &str) -> Result<(String, Vec<u8>), String> {
        let init = web_sys::RequestInit::new();
        init.set_method("GET");
        init.set_mode(web_sys::RequestMode::Cors);
        init.set_credentials(web_sys::RequestCredentials::Omit);
        init.set_referrer_policy(web_sys::ReferrerPolicy::NoReferrer);
        let req = web_sys::Request::new_with_str_and_init(url, &init).map_err(|_| "net:".to_string())?;
        let win = web_sys::window().ok_or("net:")?;
        let resp = wasm_bindgen_futures::JsFuture::from(win.fetch_with_request(&req)).await.map_err(|_| "net:".to_string())?;
        let resp: web_sys::Response = resp.dyn_into().map_err(|_| "net:".to_string())?;
        if !resp.ok() {
            return Err(format!("http:{} {}", resp.status(), resp.status_text().chars().take(60).collect::<String>()));
        }
        let ty = resp.headers().get("content-type").ok().flatten().unwrap_or_default();
        let buf = wasm_bindgen_futures::JsFuture::from(resp.array_buffer().map_err(|_| "net:".to_string())?)
            .await
            .map_err(|_| "net:".to_string())?;
        let arr = js_sys::Uint8Array::new(&buf);
        if arr.length() > 32 * 1024 * 1024 {
            return Err("http:413 too large".into());
        }
        Ok((ty, arr.to_vec()))
    }

    /// Save on the web: the bytes as a download (a Blob URL on a temporary link).
    fn download(name: &str, bytes: Vec<u8>) -> Result<Option<String>, String> {
        let fail = |e: wasm_bindgen::JsValue| format!("download: {e:?}");
        let doc = web_sys::window().and_then(|w| w.document()).ok_or("download: no document")?;
        let arr = js_sys::Uint8Array::from(bytes.as_slice());
        let parts = js_sys::Array::of1(&arr);
        let opts = web_sys::BlobPropertyBag::new();
        opts.set_type("application/octet-stream");
        let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts).map_err(fail)?;
        let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(fail)?;
        let a: web_sys::HtmlAnchorElement =
            doc.create_element("a").map_err(fail)?.dyn_into().map_err(|_| "download: no link".to_string())?;
        a.set_href(&url);
        a.set_download(name);
        a.click();
        // free the Blob once the browser has started the download
        let free = wasm_bindgen::closure::Closure::once_into_js(move || {
            let _ = web_sys::Url::revoke_object_url(&url);
        });
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(free.unchecked_ref(), 10_000);
        }
        Ok(Some(name.to_string()))
    }

    /// The Plot dialog's "Open to print": the PDF in a new tab (cadcraft.js plotUi.open); a blocked tab downloads it.
    fn open_pdf(name: &str, bytes: &[u8]) -> Result<bool, String> {
        let fail = |e: wasm_bindgen::JsValue| format!("open: {e:?}");
        let w = web_sys::window().ok_or("open: no window")?;
        let arr = js_sys::Uint8Array::from(bytes);
        let parts = js_sys::Array::of1(&arr);
        let opts = web_sys::BlobPropertyBag::new();
        opts.set_type("application/pdf");
        let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts).map_err(fail)?;
        let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(fail)?;
        let tab = w.open_with_url_and_target(&url, "_blank").ok().flatten();
        let free = wasm_bindgen::closure::Closure::once_into_js(move || {
            let _ = web_sys::Url::revoke_object_url(&url);
        });
        let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(free.unchecked_ref(), 120_000);
        if tab.is_some() {
            return Ok(true);
        }
        download(name, bytes.to_vec())?;
        Ok(false)
    }

    /// A drawing was opened this frame: its timings to the console, then the time two animation frames later (the
    /// frame drawn with it has been presented by then), so a harness can measure open → first frame.
    fn log_open(d: &cc_ui_egui::document::Loaded, frame_ms: f64) {
        let line = format!(
            "opened {} ({} bytes): read {:.1} ms · compile {:.1} ms · GPU data {:.1} ms · first frame (CPU) {:.1} ms",
            d.name, d.bytes, d.read_ms, d.compile_ms, d.geometry_ms, frame_ms
        );
        web_sys::console::log_1(&line.into());
        let Some(w) = web_sys::window() else { return };
        let second = wasm_bindgen::closure::Closure::once_into_js(move || {
            let now = web_sys::window().and_then(|w| w.performance()).map_or(f64::NAN, |p| p.now());
            web_sys::console::log_1(&format!("first frame shown at {now:.1}").into());
        });
        let first = wasm_bindgen::closure::Closure::once_into_js(move || {
            if let Some(w) = web_sys::window() {
                let _ = w.request_animation_frame(second.unchecked_ref());
            }
        });
        let _ = w.request_animation_frame(first.unchecked_ref());
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
