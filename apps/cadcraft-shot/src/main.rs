//! cadcraft-shot: drive the real UI headless (egui_kittest on wgpu; lavapipe works) and write PNGs or time frames.
//!
//!   cadcraft-shot <file.dwg|.dxf|--sample> <out.png> [--size 1600x1000] [--space N] [--theme light|dark] [--zoom F]
//!       <file> may be --hub: CadCraft Studio's start screen (nothing open), --new: an empty editable drawing, --model: the sample 3D model, --part: PartCraft's sample part; then any number of --cmd "TEXT" (typed at the command line,
//!       Enter), --click x,y (world), --mouse-click x,y (window points, a real click), --mouse-drag x1,y1:x2,y2,
//!       --mouse-move x,y, --key NAME (Enter, Escape, Backspace, Delete, A …), --mods shift|ctrl|shift+ctrl|none (held for what follows), --event copy|cut|paste:TEXT, --run ID=NAME (any UI command), --open FILE (as a drop), --save-dir DIR (where UI saves land), --pick FILE[|FILE…] (what the picker hands the next command that asks), --wait MS, --trace (print the status line after each --run) and finally --hover x,y (where the pointer rests: rubber band and snap marker)
//!       --edit: open the file for editing (CadCraft's importer); --save FORMAT:PATH (after the commands) saves it
//!       (2018 … 2000, dxf2000, dxf12); --export FORMAT:PATH exports the open model (modelcraft, glb, obj, stl, dae, ifc, dxf, dwg)
//!   cadcraft-shot pdfbench <file.pdf> [--runs N] → JSON timings of the PDF sheet view's heavy paths
//!   cadcraft-shot mkactbench <file.pdf> [--runs N] → JSON timings of MarkupCraft's editing actions (M9.16–M9.18)
//!   cadcraft-shot tkactbench <old.pdf> <new.pdf> [--runs N] → JSON timings of TakeoffCraft's actions (M9.20–M9.23)
//!   cadcraft-shot mcbench <file.modelcraft|.skp> [--runs N] → JSON timings of ModelCraft's open, panel edits, save and
//!       3D-print check (best of N; forks/tools/mcuidiff.mjs --bench runs the same steps in the 0.5 JS)
//!   cadcraft-shot mcacts <file.modelcraft|.skp> --acts "a|b|…" [--runs N] → JSON [{act, ms}]: each ModelCraft action
//!       (`model.edit=` names) timed in order, best of N fresh runs (forks/tools/mcuidiff.mjs --bench-edit, M6.19–M6.21)
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
    if src == "--hub" {
        return Ok(()); // the start screen: nothing open
    }
    if src == "--new" {
        return app.run("file.new", &Params::default());
    }
    if src == "--sample" {
        return app.run("file.sample", &Params::default());
    }
    if src == "--model" {
        return app.run("view.model.sample", &Params::default());
    }
    if src == "--part" {
        return app.run("part.sample", &Params::default());
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
        // shots render users' files: a character no bundled font has draws as a box, as in the app
        .allow_missing_glyphs()
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
        Some("pdfbench") => pdfbench(&args),
        Some("mkactbench") => mkactbench(&args),
        Some("tkactbench") => tkactbench(&args),
        Some("mcbench") => mcbench(&args),
        Some("mcacts") => mcacts(&args),
        Some("ribbon") => {
            // the ribbon / toolbar registry and styles as JSON (forks/tools/ribbondiff.mjs)
            println!("{}", cc_ui_egui::ribbon::dump());
            Ok(())
        }
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
    let export_to = arg(args, "--export").and_then(|v| v.split_once(':').map(|(f, p)| (f.to_string(), p.to_string())));
    if let Some((_, path)) = save_to.as_ref().or(export_to.as_ref()) {
        let path = path.clone();
        app.services.save = Some(Box::new(move |_name, bytes| {
            std::fs::write(&path, bytes).map_err(|e| format!("{path}: {e}"))?;
            Ok(Some(path.clone()))
        }));
    }
    // commands that read a file by its path (Insert pages from a PDF, a picture, a tool chest) read it here
    app.services.read_path = Some(Box::new(|p: &str| {
        let path = std::path::Path::new(p);
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        std::fs::read(path).map(|bytes| cc_ui_egui::ImportedFile { name, bytes, dir: None }).map_err(|e| format!("{p}: {e}"))
    }));
    if let Some(dir) = arg(args, "--save-dir") {
        let dir2 = dir.clone();
        // whatever the UI saves (exports, marked PDFs) lands in DIR under the name the app gives it
        app.services.save = Some(Box::new(move |name, bytes| {
            let file = std::path::Path::new(name).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let path = std::path::Path::new(&dir).join(file);
            std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            Ok(Some(path.display().to_string()))
        }));
        // Open to print: the PDF lands in DIR too (as if a viewer showed it)
        app.services.open_pdf = Some(Box::new(move |name, bytes| {
            let file = std::path::Path::new(name).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let path = std::path::Path::new(&dir2).join(file);
            std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            Ok(true)
        }));
    }
    // --pick FILE (repeatable): what the window's picker hands a command (XATTACH, IMAGEATTACH, IMAGE), in order;
    // several files for one IMAGE pick: --pick "a.png|b.jpg"
    let picks: Vec<String> =
        args.windows(2).filter(|w| w.first().map(String::as_str) == Some("--pick")).filter_map(|w| w.get(1).cloned()).collect();
    if !picks.is_empty() {
        let mut queue: std::collections::VecDeque<String> = picks.into();
        app.services.pick_files = Some(Box::new(move |_kind, _multiple| {
            let spec = queue.pop_front().unwrap_or_default();
            Some(
                spec.split('|')
                    .filter(|p| !p.is_empty())
                    .filter_map(|p| {
                        let name = std::path::Path::new(p).file_name()?.to_string_lossy().into_owned();
                        std::fs::read(p).ok().map(|bytes| cc_ui_egui::ImportedFile { name, bytes, dir: None })
                    })
                    .collect(),
            )
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
    let trace = args.iter().any(|a| a == "--trace");
    // modifier keys held for the pointer and key events that follow (--mods shift|ctrl|shift+ctrl|none)
    let mut mods = egui::Modifiers::NONE;
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
                let r = hs.state_mut().run(id, &Params { name, factor, ..Default::default() });
                if let Err(e) = r {
                    if !trace {
                        return Err(e);
                    }
                    // with --trace a refused command is a line too, and the run goes on
                    println!("trace\t{id}\tERROR: {}", e.replace('\n', " "));
                    i += 1;
                    continue;
                }
                hs.run_steps(2);
                if trace {
                    // one line per command: what the status line says after it (harnesses compare these)
                    println!("trace\t{id}\t{}", hs.state().message.replace('\n', " "));
                }
            }
            "--mouse-click" => {
                // a real click at window coordinates (points), through egui's input like a mouse
                if let Some((x, y)) = xy(&v) {
                    let pos = egui::pos2(x as f32, y as f32);
                    hs.event(egui::Event::PointerMoved(pos));
                    hs.run_steps(1);
                    let b = |pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: mods };
                    hs.event(b(true));
                    hs.run_steps(1);
                    hs.event(b(false));
                    hs.run_steps(2);
                }
            }
            "--mouse-drag" => {
                // a real primary-button drag at window coordinates: "x1,y1:x2,y2", in eight moves
                if let Some((a, b)) = v.split_once(':')
                    && let (Some((x1, y1)), Some((x2, y2))) = (xy(a), xy(b))
                {
                    let at = |k: f64| egui::pos2((x1 + (x2 - x1) * k) as f32, (y1 + (y2 - y1) * k) as f32);
                    let button =
                        |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: mods };
                    hs.event(egui::Event::PointerMoved(at(0.0)));
                    hs.run_steps(1);
                    hs.event(button(at(0.0), true));
                    hs.run_steps(1);
                    for i in 1..=8 {
                        hs.event(egui::Event::PointerMoved(at(f64::from(i) / 8.0)));
                        hs.run_steps(1);
                    }
                    hs.event(button(at(1.0), false));
                    hs.run_steps(2);
                }
            }
            "--mouse-path" => {
                // a real primary-button drag through several points: "x1,y1:x2,y2:x3,y3…" (a lasso), four moves a leg
                let pts: Vec<(f64, f64)> = v.split(':').filter_map(xy).collect();
                if let (Some(&(x0, y0)), true) = (pts.first(), pts.len() >= 2) {
                    let button =
                        |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: mods };
                    let p0 = egui::pos2(x0 as f32, y0 as f32);
                    hs.event(egui::Event::PointerMoved(p0));
                    hs.run_steps(1);
                    hs.event(button(p0, true));
                    hs.run_steps(1);
                    let mut last = p0;
                    for w in pts.windows(2) {
                        let (Some(&(ax, ay)), Some(&(bx, by))) = (w.first(), w.get(1)) else { continue };
                        for i in 1..=4 {
                            let k = f64::from(i) / 4.0;
                            last = egui::pos2((ax + (bx - ax) * k) as f32, (ay + (by - ay) * k) as f32);
                            hs.event(egui::Event::PointerMoved(last));
                            hs.run_steps(1);
                        }
                    }
                    if args.get(i + 2).map(String::as_str) != Some("--hold") {
                        hs.event(button(last, false));
                        hs.run_steps(2);
                    }
                }
            }
            "--wait" => {
                // keep drawing frames for MS milliseconds (background work, such as an Auto count search, lands)
                let ms: u64 = v.parse().unwrap_or(0).min(60_000);
                let until = Instant::now() + std::time::Duration::from_millis(ms);
                while Instant::now() < until {
                    hs.run_steps(1);
                    // drawn too, as the app's frames are: GPU work in paint callbacks (a rendered picture) goes on
                    hs.render()?;
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            }
            "--open" => {
                // open a file in the running app (as a drop would): with DetailCraft open an IFC imports into it
                let bytes = std::fs::read(&v).map_err(|e| format!("{v}: {e}"))?;
                let name = std::path::Path::new(&v).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                hs.state_mut().open_bytes(&name, &bytes);
                hs.run_steps(2);
            }
            "--key" => {
                // a key pressed and released (egui's key names: Enter, Escape, Backspace, Delete, A …)
                if let Some(key) = egui::Key::from_name(&v) {
                    for pressed in [true, false] {
                        hs.event(egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: mods });
                        hs.run_steps(1);
                    }
                    hs.run_steps(1);
                }
            }
            "--mods" => {
                mods = egui::Modifiers::NONE;
                for m in v.split('+') {
                    match m.trim() {
                        "shift" => mods.shift = true,
                        "ctrl" => {
                            mods.ctrl = true;
                            mods.command = true;
                        }
                        "alt" => mods.alt = true,
                        _ => {}
                    }
                }
                hs.event(egui::Event::ModifiersChanged(mods));
                hs.run_steps(1);
            }
            "--event" => {
                // what the platform sends for Ctrl+C / Ctrl+X / Ctrl+V: copy, cut, paste:TEXT (\n = a new line)
                let e = match v.split_once(':') {
                    Some(("paste", text)) => Some(egui::Event::Paste(text.replace("\\n", "\n"))),
                    _ if v == "copy" => Some(egui::Event::Copy),
                    _ if v == "cut" => Some(egui::Event::Cut),
                    _ => None,
                };
                if let Some(e) = e {
                    hs.event(e);
                    hs.run_steps(2);
                }
            }
            "--mouse-move" => {
                // the pointer moves to window coordinates (points) with no button down: rubber bands follow it
                if let Some((x, y)) = xy(&v) {
                    hs.event(egui::Event::PointerMoved(egui::pos2(x as f32, y as f32)));
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
    if let Some((f, _)) = &export_to {
        hs.state_mut().run("file.export", &Params { name: Some(f.clone()), ..Default::default() })?;
        hs.run_steps(1);
    }
    let img = hs.render()?;
    img.save(out).map_err(|e| format!("{out}: {e}"))?;
    let st = hs.state();
    println!("{out}: {}x{} · {}", img.width(), img.height(), st.message);
    Ok(())
}

/// `cadcraft-shot pdfbench <file.pdf> [--runs N]`: the sheet view's heavy paths timed (open, render page 1 at 1× and 2×,
/// save with 50 markups, TakeoffCraft's marked PDF with 50 marks), best of N, as one JSON line (forks/tools/mkbench.mjs).
fn pdfbench(args: &[String]) -> Result<(), String> {
    let f = args.get(1).ok_or("usage: cadcraft-shot pdfbench <file.pdf> [--runs N]")?;
    let runs: usize = arg(args, "--runs").and_then(|s| s.parse().ok()).unwrap_or(3).clamp(1, 50);
    let bytes = std::fs::read(f).map_err(|e| format!("{f}: {e}"))?;
    let name = std::path::Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    println!("{}", cc_ui_egui::pdfview::bench(&name, &bytes, runs)?);
    Ok(())
}

/// `cadcraft-shot mkactbench <file.pdf> [--runs N]`: MarkupCraft's editing actions timed on 200 markups (copy / paste,
/// duplicate, the list's exports, rotate and extract a page, the punch list, the chest, a legend and Save), best of N,
/// as one JSON line (forks/tools/mkactbench.mjs).
fn mkactbench(args: &[String]) -> Result<(), String> {
    let f = args.get(1).ok_or("usage: cadcraft-shot mkactbench <file.pdf> [--runs N]")?;
    let runs: usize = arg(args, "--runs").and_then(|s| s.parse().ok()).unwrap_or(3).clamp(1, 50);
    let bytes = std::fs::read(f).map_err(|e| format!("{f}: {e}"))?;
    let name = std::path::Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    println!("{}", cc_ui_egui::markupplus::bench_actions(&name, &bytes, runs)?);
    Ok(())
}

/// `cadcraft-shot tkactbench <old.pdf> <new.pdf> [--runs N]`: TakeoffCraft's M9.20–M9.23 actions timed (copy / paste,
/// a corner drag, a typical group, notes, Find changes, Move takeoff here, the marked PDF, 3D + GLB, delete), best of N,
/// as one JSON line (forks/tools/tkactbench.mjs).
fn tkactbench(args: &[String]) -> Result<(), String> {
    let (Some(a), Some(b)) = (args.get(1), args.get(2)) else {
        return Err("usage: cadcraft-shot tkactbench <old.pdf> <new.pdf> [--runs N]".into());
    };
    let runs: usize = arg(args, "--runs").and_then(|s| s.parse().ok()).unwrap_or(3).clamp(1, 50);
    let ba = std::fs::read(a).map_err(|e| format!("{a}: {e}"))?;
    let bb = std::fs::read(b).map_err(|e| format!("{b}: {e}"))?;
    let name = |f: &str| std::path::Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    println!("{}", cc_ui_egui::tkbench::bench((&name(a), &ba), (&name(b), &bb), runs)?);
    Ok(())
}

/// `cadcraft-shot mcbench <file> [--runs N]`: ModelCraft's open (parse + load, or SketchUp import), the edits a session
/// makes from the panels (select all, make group, tag, paint a material, add a scene, hide and unhide all, undo), Save
/// (the file's text) and Check for 3D printing, each the best of N, as one JSON line.
fn mcbench(args: &[String]) -> Result<(), String> {
    let f = args.get(1).ok_or("usage: cadcraft-shot mcbench <file> [--runs N]")?;
    let runs: usize = arg(args, "--runs").and_then(|s| s.parse().ok()).unwrap_or(3).clamp(1, 50);
    let bytes = std::fs::read(f).map_err(|e| format!("{f}: {e}"))?;
    let name = std::path::Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut best = [f64::INFINITY; 4];
    let mut size = 0usize;
    let mut faces = 0usize;
    for _ in 0..runs {
        let mut app = UiApp::default();
        let t = Instant::now();
        app.open_bytes(&name, &bytes);
        let open = t.elapsed().as_secs_f64() * 1000.0;
        if std::env::var_os("MCBENCH_SPLIT").is_some() {
            eprintln!("open total {open:.1} ms; {}", app.open_split.clone());
        }
        if let Some(e) = &app.error {
            return Err(e.clone());
        }
        let mc = app.mc.as_mut().ok_or("mcbench: no model opened")?;
        faces = mc.tools.host.model.defs.values().map(|d| d.faces.len()).sum();
        let t = Instant::now();
        for a in
            ["select-all", "make-group", "tag-add", "tag-set:Tag", "paint:mat:wood-oak", "scene-add", "hide", "unhide-all", "undo", "redo"]
        {
            mc.act(a)?;
        }
        let edit = t.elapsed().as_secs_f64() * 1000.0;
        let t = Instant::now();
        let text = mc.save_text();
        let save = t.elapsed().as_secs_f64() * 1000.0;
        size = text.len();
        mc.act("select-none")?;
        let t = Instant::now();
        mc.act("print-check")?;
        let print = t.elapsed().as_secs_f64() * 1000.0;
        for (b, v) in best.iter_mut().zip([open, edit, save, print]) {
            *b = b.min(v);
        }
    }
    println!(
        "{{\"file\":{:?},\"faces\":{faces},\"open_ms\":{:.2},\"edit_ms\":{:.2},\"save_ms\":{:.2},\"print_ms\":{:.2},\"saved_bytes\":{size}}}",
        name, best[0], best[1], best[2], best[3]
    );
    Ok(())
}

fn mcacts(args: &[String]) -> Result<(), String> {
    let f = args.get(1).ok_or("usage: cadcraft-shot mcacts <file> --acts \"a|b|…\" [--runs N]")?;
    let acts: Vec<String> = arg(args, "--acts").ok_or("mcacts: --acts is required")?.split('|').map(str::to_string).collect();
    let runs: usize = arg(args, "--runs").and_then(|s| s.parse().ok()).unwrap_or(3).clamp(1, 50);
    let bytes = std::fs::read(f).map_err(|e| format!("{f}: {e}"))?;
    let name = std::path::Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut best = vec![f64::INFINITY; acts.len()];
    let mut msgs = vec![String::new(); acts.len()];
    for _ in 0..runs {
        let mut app = UiApp::default();
        app.open_bytes(&name, &bytes);
        if let Some(e) = &app.error {
            return Err(e.clone());
        }
        let mc = app.mc.as_mut().ok_or("mcacts: no model opened")?;
        for (k, a) in acts.iter().enumerate() {
            let t = Instant::now();
            let r = mc.act(a);
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            mc.take_tool_message();
            if let (Some(b), Some(m)) = (best.get_mut(k), msgs.get_mut(k)) {
                *b = b.min(ms);
                *m = r.err().unwrap_or_else(|| mc.message.clone());
            }
        }
    }
    let rows: Vec<String> = acts
        .iter()
        .zip(best.iter().zip(msgs.iter()))
        .map(|(a, (ms, m))| format!("{{\"act\":{a:?},\"ms\":{ms:.3},\"msg\":{m:?}}}"))
        .collect();
    println!("[{}]", rows.join(","));
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
    let Some(src) = args.get(1) else {
        return Err("usage: cadcraft-shot bench <file|--sample> [--frames N] [--run ID=NAME]… [--orbit cx,cy,cz,r]".into());
    };
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
    // --run ID=NAME (any number, before the first frame): e.g. model.edit=shadows:on
    for (k, v) in args.iter().enumerate() {
        if v == "--run"
            && let Some(x) = args.get(k + 1)
        {
            let (id, name) = x.split_once('=').map_or((x.as_str(), None), |(a, b)| (a, Some(b.to_string())));
            hs.state_mut().run(id, &Params { name, ..Default::default() })?;
        }
    }
    // --orbit cx,cy,cz,r: each frame moves the 3D camera around that point (ModelCraft) instead of zooming
    let orbit: Option<Vec<f64>> = arg(args, "--orbit").map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect());
    let t = Instant::now();
    hs.step();
    hs.render()?;
    let first = t.elapsed().as_secs_f64() * 1000.0;
    let mut times = Vec::with_capacity(frames);
    let mut ui_times = Vec::with_capacity(frames);
    for i in 0..frames {
        if let Some(&[cx, cy, cz, r]) = orbit.as_deref() {
            let a = i as f64 * 0.15;
            let cam = format!("cam:{},{},{},{cx},{cy},{cz}", cx + r * a.cos(), cy + r * a.sin(), cz + r * 0.6);
            hs.state_mut().run("model.edit", &Params { name: Some(cam), ..Default::default() })?;
        } else {
            let z = if i % 2 == 0 { 1.15 } else { 1.0 / 1.1 };
            hs.state_mut().run("view.zoom", &Params { factor: Some(z), ..Default::default() })?;
        }
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
