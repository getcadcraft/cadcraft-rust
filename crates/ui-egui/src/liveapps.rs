//! Studio Live for the apps whose window state lives in [`UiApp`] (every app but DetailCraft, which keeps its session
//! in its own view): one session per window (`UiApp::live`), the app it is for, routing of the `<prefix>.live.*`
//! commands, each frame's carrying of messages, the app's own steps sent after each command, and per-person undo.
//! Each app's adapter is in its own `<app>live.rs`; this file only connects them to the window.

use crate::UiApp;

/// Which app a `<prefix>.live.*` command is for.
pub fn app_of(id: &str) -> Option<&'static str> {
    let (p, rest) = id.split_once('.')?;
    if !rest.starts_with("live.") {
        return None;
    }
    Some(match p {
        "takeoff" => "takeoffcraft",
        "markup" => "markupcraft",
        "part" => "partcraft",
        "civil" => "civilcraft",
        "model" => "modelcraft",
        "bim" => "bimcraft",
        _ => return None,
    })
}

/// A `<prefix>.live.*` command (None: not one of these apps' live commands).
pub fn run(app: &mut UiApp, id: &str, name: &str) -> Option<Result<(), String>> {
    // BimCraft's Undo / Redo while live: the person's own steps (bimcraft live.js `watchEditor`)
    if matches!(id, "bim.undo" | "bim.redo") && app.live.as_ref().is_some_and(|s| s.app == "bimcraft") {
        return Some(crate::bimlive::BimLive(app).reverse(id == "bim.undo"));
    }
    let which = app_of(id)?;
    if let Some(s) = app.live.as_ref()
        && s.app != which
        && !id.ends_with(".window")
    {
        return Some(Err(format!("{id}: this window is in a {} live session", s.app)));
    }
    if id.ends_with(".live.state") {
        // headless checks: the shared document (live.js `hash()`) saved as <app>.live.json
        let st = match which {
            "takeoffcraft" => crate::live::LiveView::live_state(&mut crate::takeofflive::TkLive(app)),
            "markupcraft" => crate::live::LiveView::live_state(&mut crate::markuplive::MkLive(app)),
            "partcraft" => crate::live::LiveView::live_state(&mut crate::partlive::PartLive(app)),
            "civilcraft" => crate::live::LiveView::live_state(&mut crate::civillive::CvLive(app)),
            "modelcraft" => crate::live::LiveView::live_state(&mut crate::modellive::McLive(app)),
            "bimcraft" => crate::live::LiveView::live_state(&mut crate::bimlive::BimLive(app)),
            _ => None,
        };
        let Some(st) = st else { return Some(Err(format!("{id}: nothing open"))) };
        let text = crate::livestate::canonical(&st);
        let Some(save) = app.services.save.as_mut() else { return Some(Err(format!("{id}: saving is not available here"))) };
        return Some(save(&format!("{which}.live.json"), text.into_bytes()).map(|_| ()));
    }
    Some(match which {
        "takeoffcraft" => crate::live::run(&mut crate::takeofflive::TkLive(app), id, name),
        "markupcraft" => crate::live::run(&mut crate::markuplive::MkLive(app), id, name),
        "partcraft" => crate::live::run(&mut crate::partlive::PartLive(app), id, name),
        "civilcraft" => crate::live::run(&mut crate::civillive::CvLive(app), id, name),
        "modelcraft" => crate::live::run(&mut crate::modellive::McLive(app), id, name),
        "bimcraft" => crate::live::run(&mut crate::bimlive::BimLive(app), id, name),
        _ => Err(format!("unknown command {id}")),
    })
}

/// After every command and every frame: the app's new steps go out, what came in lands, undo / redo asked for while
/// live go to the session.
pub fn pump(app: &mut UiApp) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Some(which) = app.live.as_ref().map(|s| s.app) else { return };
        match which {
            "takeoffcraft" => {
                let mut v = crate::takeofflive::TkLive(app);
                crate::live::poll(&mut v);
                v.reverse_asked();
                let can = app.live.as_ref().map_or((false, false), |s| (s.doc.can_undo(), s.doc.can_redo()));
                if let Some(p) = app.pdf.as_mut() {
                    p.takeoff.live_can = can;
                }
            }
            "markupcraft" => {
                let mut v = crate::markuplive::MkLive(app);
                crate::live::poll(&mut v);
                v.reverse_asked();
                let can = app.live.as_ref().map_or((false, false), |s| (s.doc.can_undo(), s.doc.can_redo()));
                if let Some(p) = app.pdf.as_mut() {
                    p.live_can = can;
                }
            }
            "partcraft" => {
                let mut v = crate::partlive::PartLive(app);
                crate::live::poll(&mut v);
                v.reverse_asked();
                let can = app.live.as_ref().map_or((false, false), |s| (s.doc.can_undo(), s.doc.can_redo()));
                if let Some(p) = app.part.as_mut() {
                    p.live_can = can;
                }
            }
            "civilcraft" => {
                let mut v = crate::civillive::CvLive(app);
                crate::live::poll(&mut v);
                v.reverse_asked();
                let can = app.live.as_ref().map_or((false, false), |s| (s.doc.can_undo(), s.doc.can_redo()));
                if let Some(c) = app.civil.as_mut() {
                    c.live_can = can;
                }
            }
            "modelcraft" => {
                let mut v = crate::modellive::McLive(app);
                crate::live::poll(&mut v);
                v.reverse_asked();
            }
            "bimcraft" => crate::live::poll(&mut crate::bimlive::BimLive(app)),
            _ => {}
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = app;
}

/// The Live window of the app on screen, and a repaint while a session runs.
pub fn frame(app: &mut UiApp, ctx: &egui::Context) {
    #[cfg(target_arch = "wasm32")]
    let _ = ctx;
    if let Some(p) = app.pdf.as_mut()
        && (std::mem::take(&mut p.takeoff.want_live) | std::mem::take(&mut p.want_live))
    {
        app.live_window = true;
    }
    pump(app);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let tk = app.tokens;
        let mut acts: Vec<(String, String)> = vec![];
        let which = app.live.as_ref().map(|s| s.app).or(app.live_window.then(|| current_app(app)).flatten());
        match which {
            Some("takeoffcraft") => crate::live::window(&mut crate::takeofflive::TkLive(app), ctx, &tk, &mut acts),
            Some("markupcraft") => crate::live::window(&mut crate::markuplive::MkLive(app), ctx, &tk, &mut acts),
            Some("partcraft") => crate::live::window(&mut crate::partlive::PartLive(app), ctx, &tk, &mut acts),
            Some("civilcraft") => crate::live::window(&mut crate::civillive::CvLive(app), ctx, &tk, &mut acts),
            Some("modelcraft") => crate::live::window(&mut crate::modellive::McLive(app), ctx, &tk, &mut acts),
            Some("bimcraft") => crate::live::window(&mut crate::bimlive::BimLive(app), ctx, &tk, &mut acts),
            _ => {}
        }
        for (id, n) in acts {
            if let Some(Err(e)) = run(app, &id, &n) {
                app.message = e;
            }
        }
        if app.live.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
    }
}

/// The app the window shows now (for the Live window before a session starts).
pub fn current_app(app: &UiApp) -> Option<&'static str> {
    Some(app.active_app()).filter(|a| *a != "studio" && *a != "detailcraft")
}
