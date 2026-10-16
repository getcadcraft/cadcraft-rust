//! cadcraft-shot: drive the real UI headless (egui_kittest on wgpu; lavapipe works) and write PNGs or time frames.
//!
//!   cadcraft-shot <file.dwg|.dxf|--sample> <out.png> [--size 1600x1000] [--space N] [--theme light|dark] [--zoom F]
//!       <file> may be --new: an empty editable drawing, --model: the sample 3D model; then any number of --cmd "TEXT" (typed at the command line,
//!       Enter), --click x,y (world), --mouse-click x,y (window points, a real click) and finally --hover x,y (where the pointer rests: rubber band and snap marker)
//!       --edit: open the file for editing (CadCraft's importer); --save FORMAT:PATH (after the commands) saves it
//!       (2018 … 2000, dxf2000, dxf12)
//!   cadcraft-shot bench <file.dwg|.dxf|--sample> [--frames N] [--size WxH] [--space N] [--edit]
//!       → JSON {open_ms, first_frame_ms, frame_ms: [...], spaces, segments …}; each frame pans/zooms the view.
//!       --edit: open for editing, then also time one LINE (typed) from Enter to the redrawn frame (edit_ms)
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
    if src == "--new" {
        return app.run("file.new", &Params::default());
    }
    if src == "--sample" {
        return app.run("file.sample", &Params::default());
    }
    if src == "--model" {
        return app.run("view.model.sample", &Params::default());
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
    let r = match args.first().map(String::as_str) {
        Some("bench") => bench(&args),
        Some("demo3d") => demo3d(&args),
        _ => shot(&args),
    };
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
    if args.iter().any(|a| a == "--edit") {
        app.run("file.edit", &Params::default())?;
    }
    let save_to = arg(args, "--save").and_then(|v| v.split_once(':').map(|(f, p)| (f.to_string(), p.to_string())));
    if let Some((_, path)) = &save_to {
        let path = path.clone();
        app.services.save = Some(Box::new(move |_name, bytes| {
            std::fs::write(&path, bytes).map_err(|e| format!("{path}: {e}"))?;
            Ok(Some(path.clone()))
        }));
    }
    if let Some(i) = arg(args, "--space").and_then(|s| s.parse().ok()) {
        app.run("space.select", &Params { index: Some(i), ..Default::default() })?;
    }
    let zoom = arg(args, "--zoom").and_then(|s| s.parse::<f64>().ok());
    let mut hs = harness(app, w, h);
    hs.run_steps(2);
    let xy = |v: &str| -> Option<(f64, f64)> {
        let (a, b) = v.split_once(',')?;
        Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
    };
    let mut i = 0;
    while let Some(a) = args.get(i) {
        let v = args.get(i + 1).cloned().unwrap_or_default();
        match a.as_str() {
            "--cmd" => {
                hs.state_mut().run("cmd.line", &Params { name: Some(v), ..Default::default() })?;
                hs.run_steps(1);
            }
            "--click" => {
                if let Some((x, y)) = xy(&v) {
                    hs.state_mut().click_world(x, y);
                    hs.run_steps(1);
                }
            }
            "--run" => {
                // any UI command: --run "model.toggle=c:IFCSLAB" (name after '=')
                let (id, name) = v.split_once('=').map_or((v.as_str(), None), |(a, b)| (a, Some(b.to_string())));
                let factor = name.as_deref().and_then(|n| n.parse::<f64>().ok());
                hs.state_mut().run(id, &Params { name, factor, ..Default::default() })?;
                hs.run_steps(2);
            }
            "--mouse-click" => {
                // a real click at window coordinates (points), through egui's input like a mouse
                if let Some((x, y)) = xy(&v) {
                    let pos = egui::pos2(x as f32, y as f32);
                    hs.event(egui::Event::PointerMoved(pos));
                    hs.run_steps(1);
                    let b = |pressed| egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    };
                    hs.event(b(true));
                    hs.run_steps(1);
                    hs.event(b(false));
                    hs.run_steps(2);
                }
            }
            "--hover" => {
                if let Some((x, y)) = xy(&v) {
                    hs.state_mut().hover_world(x, y);
                    hs.run_steps(1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    if let Some(z) = zoom {
        hs.state_mut().run("view.zoom", &Params { factor: Some(z), ..Default::default() })?;
        hs.run_steps(1);
    }
    if let Some((f, _)) = &save_to {
        hs.state_mut().run("file.save", &Params { name: Some(f.clone()), ..Default::default() })?;
        hs.run_steps(1);
    }
    let img = hs.render()?;
    img.save(out).map_err(|e| format!("{out}: {e}"))?;
    let st = hs.state();
    println!("{out}: {}x{} · {}", img.width(), img.height(), st.message);
    Ok(())
}

/// `cadcraft-shot demo3d <out.png> [--n N] [--survey] [--frames F] [--size WxH]`: the 3D viewport on a test scene of
/// N×N instanced boxes (at x = 500 000, y = 4 000 000 with --survey); writes a PNG and times F orbit frames.
fn demo3d(args: &[String]) -> Result<(), String> {
    use cc_ui_egui::view3d::{View3, demo_scene, show};
    let out = args.get(1).ok_or("usage: cadcraft-shot demo3d <out.png> [--n N] [--survey] [--frames F]")?.clone();
    let n: usize = arg(args, "--n").and_then(|s| s.parse().ok()).unwrap_or(12);
    let frames: usize = arg(args, "--frames").and_then(|s| s.parse().ok()).unwrap_or(0);
    let (w, h) = arg(args, "--size")
        .and_then(|s| s.split_once('x').map(|(a, b)| (a.parse().unwrap_or(1200.0), b.parse().unwrap_or(800.0))))
        .unwrap_or((1200.0, 800.0));
    let origin = if args.iter().any(|a| a == "--survey") { [500_000.0, 4_000_000.0, 120.0] } else { [0.0; 3] };
    let scene = demo_scene(n, origin);
    let rs = egui_kittest::wgpu::create_render_state(egui_kittest::wgpu::default_wgpu_setup(), egui_wgpu_options());
    cc_ui_egui::install_gpu(&rs);
    let renderer = egui_kittest::wgpu::WgpuTestRenderer::from_render_state(rs);
    let mut hs = egui_kittest::Harness::builder().with_size(egui::vec2(w, h)).with_pixels_per_point(1.0).renderer(renderer).build_ui_state(
        move |ui, v: &mut View3| {
            let rect = ui.max_rect();
            show(ui, rect, &scene, v);
        },
        View3::default(),
    );
    hs.step();
    let img = hs.render()?;
    img.save(&out).map_err(|e| format!("{out}: {e}"))?;
    let tris = (n * n * 12 + 12) as u64;
    let mut times = Vec::new();
    for _ in 0..frames {
        hs.state_mut().cam.orbit(9.0, 2.0);
        let t = Instant::now();
        hs.step();
        hs.render()?;
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    let best = times.first().copied().unwrap_or(f64::NAN);
    let median = times.get(times.len() / 2).copied().unwrap_or(f64::NAN);
    println!(
        "{out}: {}x{} · {} instances · {tris} triangles · orbit frame best {best:.2} ms, median {median:.2} ms",
        img.width(),
        img.height(),
        n * n + 1
    );
    Ok(())
}

fn bench(args: &[String]) -> Result<(), String> {
    let Some(src) = args.get(1) else { return Err("usage: cadcraft-shot bench <file|--sample> [--frames N]".into()) };
    let frames: usize = arg(args, "--frames").and_then(|s| s.parse().ok()).unwrap_or(30);
    let (w, h) = size(args);
    let mut app = UiApp::default();
    let t = Instant::now();
    load(&mut app, src)?;
    let edit = args.iter().any(|a| a == "--edit");
    if edit {
        app.run("file.edit", &Params::default())?;
    }
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
    let mut ui_times = Vec::with_capacity(frames);
    for i in 0..frames {
        let z = if i % 2 == 0 { 1.15 } else { 1.0 / 1.1 };
        hs.state_mut().run("view.zoom", &Params { factor: Some(z), ..Default::default() })?;
        let t = Instant::now();
        hs.step();
        hs.render()?;
        times.push(t.elapsed().as_secs_f64() * 1000.0);
        ui_times.push(hs.state().frame_ms);
    }
    let mut edit_ms = String::from("null");
    if edit {
        // a LINE across the view: the command runs, the display model is rebuilt, the GPU data re-made and drawn
        let (cx, cy) = hs.state().views.get(hs.state().space).copied().flatten().map_or((0.0, 0.0), |v| (v.cx, v.cy));
        hs.state_mut().run("cmd.line", &Params { name: Some("LINE".into()), ..Default::default() })?;
        hs.state_mut().run("cmd.line", &Params { name: Some(format!("{cx},{cy}")), ..Default::default() })?;
        let t = Instant::now();
        hs.state_mut().run("cmd.line", &Params { name: Some("@100,100".into()), ..Default::default() })?;
        hs.step();
        hs.render()?;
        edit_ms = format!("{:.2}", t.elapsed().as_secs_f64() * 1000.0);
        if let Some(ed) = hs.state().editor.as_ref() {
            eprintln!("display after the edit: {:?}", ed.display);
        }
        hs.state_mut().run("cmd.line", &Params { name: Some(String::new()), ..Default::default() })?;
    }
    let st = hs.state();
    let (segs, fills) = st.doc.as_ref().map_or((0, 0), |d| d.geometry.get(st.space).map_or((0, 0), |g| (g.segments(), g.fill_vertices())));
    eprintln!("space {} · segments {segs} · fill vertices {fills}", st.space);
    let list: Vec<String> = times.iter().map(|t| format!("{t:.2}")).collect();
    let ui_list: Vec<String> = ui_times.iter().map(|t| format!("{t:.2}")).collect();
    println!(
        r#"{{"open_ms":{open_ms:.2},"edit_ms":{edit_ms},"first_frame_ms":{first:.2},"frame_ms":[{}],"ui_ms":[{}],"texts":{},"message":"{}"}}"#,
        list.join(","),
        ui_list.join(","),
        st.last_stats.texts,
        st.message.replace('"', "'")
    );
    Ok(())
}
