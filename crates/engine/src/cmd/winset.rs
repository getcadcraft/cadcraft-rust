//! The window's own settings and dialogs (commands.js): SPLIT (split screen from a sheet too), LINKVIEWS (panes pan
//! and zoom together), OPENIN3D (the drawing to the 3D modeling app; this window's own 3D tab), LASSOMODE (press and drag: lasso or
//! rectangle), SHORTCUTS (keys for any command), COMPARESETTINGS (styles, layers, sheets and units side by side). Each
//! asks the host; a host without the feature prints 0.5's line for that case.

use crate::engine::{Ctx, Opts, R, Reply};
use crate::input::Def;

/// SPLIT: the split-screen panes of VPORTS, from a sheet too (View › More uses it, so its items never turn into MVIEW).
pub async fn split(c: Ctx) -> R {
    crate::cmd::layout::split_panes(c).await
}

/// LINKVIEWS (SYNCVIEWS): pan and zoom move together in every pane.
pub async fn linkviews(c: Ctx) -> R {
    let on = c.sh().host.borrow().link_views();
    let Some(on) = on else {
        c.print(&c.t("cmd.vports.browser"));
        return Ok(());
    };
    let options = vec![c.opt("ON", "ON"), c.opt("OFF", "OFF")];
    let r = c
        .keyword(
            c.t("cmd.linkviews.ask"),
            Opts { options, default: Some(Def::Str(if on { "OFF" } else { "ON" }.into())), ..Opts::default() },
        )
        .await?;
    let k = match &r {
        Reply::Opt(o) | Reply::Str(o) => o.to_uppercase(),
        _ => String::new(),
    };
    if k != "ON" && k != "OFF" {
        return Ok(());
    }
    c.sh().host.borrow_mut().set_link_views(k == "ON");
    c.print(&c.t(if k == "ON" { "cmd.linkviews.on" } else { "cmd.linkviews.off" }));
    Ok(())
}

/// OPENIN3D: the drawing to the 3D modeling app (only where that app takes a handed-over drawing).
pub async fn openin3d(c: Ctx) -> R {
    let r = c.sh().host.borrow_mut().open_in_3d();
    match r {
        // no other app takes the drawing here: the window's own 3D tab shows it (the same drawing, one history)
        None if c.sh().open3d() => c.print("The drawing is open on the 3D tab: the same drawing, so changes there are changes here."),
        None => c.print(&c.t("cmd.open3d.none")),
        Some(Ok(app)) => c.print(&c.tv("cmd.open3d.done", &[("app", app)])),
        Some(Err((code, error))) => {
            let key = match code.as_str() {
                "blocked" => "cmd.open3d.blocked",
                "storage" => "cmd.open3d.storage",
                _ => "cmd.open3d.failed",
            };
            c.print(&c.tv(key, &[("error", error)]));
        }
    }
    Ok(())
}

/// LASSOMODE: press and drag draws a lasso (1, the default) or a rectangle (0).
pub async fn lassomode(c: Ctx) -> R {
    let cur = c.sh().host.borrow().lasso();
    let Some(cur) = cur else {
        c.print(&c.t("cmd.lassomode.browser"));
        return Ok(());
    };
    let n = c.number(c.t("cmd.lassomode.ask"), Opts { default: Some(Def::Num(if cur { 1.0 } else { 0.0 })), ..Opts::default() }).await?;
    match n {
        Reply::Num(v) if v == 0.0 || v == 1.0 => c.sh().host.borrow_mut().set_lasso(v == 1.0),
        _ => c.print(&c.t("cmd.lassomode.range")),
    }
    Ok(())
}

/// SHORTCUTS (KEYS): the editor of keys for any command.
pub async fn shortcuts(c: Ctx) -> R {
    let opened = c.sh().host.borrow_mut().shortcuts();
    if !opened {
        c.print(&c.t("keys.browser"));
    }
    Ok(())
}

/// SHEETSET (SSM, SHEETSETMANAGER): the Sheet Set panel.
pub async fn sheetset(c: Ctx) -> R {
    let r = c.sh().host.borrow_mut().sheet_set();
    match r {
        None => c.print(&c.t("sheetset.cmd.browser")),
        Some(Some(name)) => c.print(&c.tv("sheetset.cmd.open", &[("name", name)])),
        Some(None) => c.print(&c.t("sheetset.cmd.none")),
    }
    Ok(())
}

/// COMPARESETTINGS (CMPSET): the Compare settings dialog.
pub async fn comparesettings(c: Ctx) -> R {
    let opened = c.sh().host.borrow_mut().compare_settings();
    if !opened {
        c.print(&c.t("cmd.cmp.browser"));
    }
    Ok(())
}
