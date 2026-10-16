//! cadcraft-shot: drive the real UI headless (egui_kittest on wgpu; lavapipe works) and write PNGs or time frames.
//!
//!   cadcraft-shot <file.dwg|.dxf|--sample> <out.png> [--size 1600x1000] [--space N] [--theme light|dark] [--zoom F]
//!   cadcraft-shot bench <file.dwg|.dxf|--sample> [--frames N] [--size WxH] [--space N]
//!       → JSON {open_ms, first_frame_ms, frame_ms: [...], spaces, segments …}; each frame pans/zooms the view.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::process::ExitCode;
use std::time::Instant;

use cc_ui_egui::{Params, UiApp};

fn arg(args: &[String], k: &str) -> Option<String> {
    args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned()
}

fn size(args: &[String]) -> (f32, f32) {
    arg(args, "--size")
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or((1600.0, 1000.0))
}

fn load(app: &mut UiApp, src: &str) -> Result<(), String> {
    if src == "--sample" {
        return app.run("file.sample", &Params::default());
    }
    let bytes = std::fs::read(src).map_err(|e| format!("{src}: {e}"))?;
    let name = std::path::Path::new(src).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    app.open_bytes(&name, &bytes);
    match &app.error {
        Some(e) => Err(e.clone()),
        None => Ok(()),
    }
}

fn harness<'a>(app: UiApp, w: f32, h: f32) -> egui_kittest::Harness<'a, UiApp> {
    let rs = egui_kittest::wgpu::create_render_state(egui_kittest::wgpu::default_wgpu_setup(), egui_wgpu_options());
    cc_ui_egui::install_gpu(&rs);
    let renderer = egui_kittest::wgpu::WgpuTestRenderer::from_render_state(rs);
    egui_kittest::Harness::builder()
        .with_size(egui::vec2(w, h))
        .with_pixels_per_point(1.0)
        .renderer(renderer)
        .build_ui_state(|ui, app: &mut UiApp| app.ui(ui), app)
}

fn egui_wgpu_options() -> egui_wgpu::RendererOptions {
    egui_wgpu::RendererOptions::PREDICTABLE
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = if args.first().map(String::as_str) == Some("bench") { bench(&args) } else { shot(&args) };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cadcraft-shot: {e}");
            ExitCode::FAILURE
        }
    }
}

fn shot(args: &[String]) -> Result<(), String> {
    let (Some(src), Some(out)) = (args.first(), args.get(1)) else {
        return Err("usage: cadcraft-shot <file|--sample> <out.png> [--size WxH] [--space N] [--theme light|dark]".into());
    };
    let (w, h) = size(args);
    let mut app = UiApp::default();
    if arg(args, "--theme").as_deref() == Some("light") {
        app.run("theme.toggle", &Params::default())?;
    }
    load(&mut app, src)?;
    if let Some(i) = arg(args, "--space").and_then(|s| s.parse().ok()) {
        app.run("space.select", &Params { index: Some(i), ..Default::default() })?;
    }
    let zoom = arg(args, "--zoom").and_then(|s| s.parse::<f64>().ok());
    let mut hs = harness(app, w, h);
    hs.run_steps(2);
    if let Some(z) = zoom {
        hs.state_mut().run("view.zoom", &Params { factor: Some(z), ..Default::default() })?;
        hs.run_steps(1);
    }
    let img = hs.render()?;
    img.save(out).map_err(|e| format!("{out}: {e}"))?;
    let st = hs.state();
    println!("{out}: {}x{} · {}", img.width(), img.height(), st.message);
    Ok(())
}

fn bench(args: &[String]) -> Result<(), String> {
    let Some(src) = args.get(1) else { return Err("usage: cadcraft-shot bench <file|--sample> [--frames N]".into()) };
    let frames: usize = arg(args, "--frames").and_then(|s| s.parse().ok()).unwrap_or(30);
    let (w, h) = size(args);
    let mut app = UiApp::default();
    let t = Instant::now();
    load(&mut app, src)?;
    let open_ms = t.elapsed().as_secs_f64() * 1000.0;
    if let Some(i) = arg(args, "--space").and_then(|s| s.parse().ok()) {
        app.run("space.select", &Params { index: Some(i), ..Default::default() })?;
    }
    let mut hs = harness(app, w, h);
    let t = Instant::now();
    hs.step();
    hs.render()?;
    let first = t.elapsed().as_secs_f64() * 1000.0;
    let mut times = Vec::with_capacity(frames);
    for i in 0..frames {
        let z = if i % 2 == 0 { 1.15 } else { 1.0 / 1.1 };
        hs.state_mut().run("view.zoom", &Params { factor: Some(z), ..Default::default() })?;
        let t = Instant::now();
        hs.step();
        hs.render()?;
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let st = hs.state();
    let list: Vec<String> = times.iter().map(|t| format!("{t:.2}")).collect();
    println!(
        r#"{{"open_ms":{open_ms:.2},"first_frame_ms":{first:.2},"frame_ms":[{}],"texts":{},"message":"{}"}}"#,
        list.join(","),
        st.last_stats.texts,
        st.message.replace('"', "'")
    );
    Ok(())
}
